#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

ruby -ryaml - "$ROOT" "$WORK" <<'RUBY'
root, work = ARGV
workflow = YAML.load_file("#{root}/.github/workflows/slashing-tests.yml")
trigger = workflow['on'] || workflow[true]
abort 'FAIL: The formal workflow does not trigger on pull requests.' unless trigger.key?('pull_request')
job = workflow.fetch('jobs').fetch('tla-model-check')
unless job['if'].nil? || job['if'] == true
  abort 'FAIL: The TLA+ invariant job is not enabled for every pull request.'
end
budget = "${{ (github.event_name == 'schedule' || github.event_name == 'workflow_dispatch') && 240 || 15 }}"
abort 'FAIL: The formal job must bound PR runs to 15 minutes and retain the nightly budget.' unless job['timeout-minutes'] == budget
step = job.fetch('steps').find { |item| item.fetch('run', '').include?('scripts/ci/check-formal-invariants.sh') }
abort 'FAIL: The formal verification command is missing.' unless step
exhaustive = "${{ (github.event_name == 'workflow_dispatch' && inputs.run_exhaustive) && '1' || '0' }}"
abort 'FAIL: Only manual dispatch can select exhaustive verification.' unless step.fetch('env').fetch('RUN_EXHAUSTIVE_TLA') == exhaustive
abort 'FAIL: Formal errors must fail the job.' if job['continue-on-error'] || step['continue-on-error'] || step['if']
File.write("#{work}/workflow-run.sh", step.fetch('run'))
RUBY

mkdir -p "$WORK/bin" "$WORK/repo/scripts/ci" "$WORK/repo/formal"
cp "$ROOT/scripts/ci/check-tla-invariants.sh" "$ROOT/scripts/ci/check-formal-invariants.sh" "$WORK/repo/scripts/ci/"
cp -R "$ROOT/formal/tlaplus" "$WORK/repo/formal/"
cat >"$WORK/bin/tlc" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
config=""
while (($#)); do
    if [[ "$1" == -config ]]; then
        config="$2"
        break
    fi
    shift
done
if [[ "$config" == MC_CarrierIndex.cfg && "$TEST_TLC_FAIL" == 1 ]]; then
    printf 'Error: Invariant IndexCompleteForWindow is violated.\n'
    exit 12
fi
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
    *) printf 'Model checking completed. No error has been found.\n'; exit 0 ;;
esac
printf 'Error: Invariant %s is violated.\n' "$invariant"
exit 12
SH
chmod +x "$WORK/bin/tlc"

pr_configs=(
    replay_liveness/MC_ReplayHotLoop
    carrier_index/MC_CarrierIndex
    soak_disk/MC_SoakDisk
    soak_disk/MC_SoakDisk_floor_only_pre_fix
    soak_disk/MC_DiskProbeAdmission
    soak_disk/MC_DiskProbeAdmission_missing_sample_pre_fix
    soak_disk/MC_DiskSampleValidation
    soak_disk/MC_DiskSampleValidation_numeric_prefix_pre_fix
    soak_disk/MC_ActiveDiskProbe
    soak_disk/MC_ActiveDiskProbe_missing_pre_fix
    soak_disk/MC_DiskEmergencyRecord
    soak_disk/MC_DiskEmergencyRecord_stop_first_pre_fix
    soak_disk/MC_GuardianSupervision
    soak_disk/MC_GuardianSupervision_unwatched_pre_fix
    soak_disk/MC_DiskProbeDeadline
    soak_disk/MC_DiskProbeDeadline_unbounded_pre_fix
    soak_disk/MC_DiskDiagnosticDeadline
    soak_disk/MC_DiskDiagnosticDeadline_per_root_pre_fix
    soak_disk/MC_DiskBreachRestart
    soak_disk/MC_DiskBreachRestart_clear_pre_fix
    soak_disk/MC_DiskStopDeadline
    soak_disk/MC_DiskStopDeadline_unbounded_pre_fix
    soak_disk/MC_GuardianAdmission
    soak_disk/MC_GuardianAdmission_unchecked_pre_fix
    soak_disk/MC_BenchmarkBreachAdmission
    soak_disk/MC_BenchmarkBreachAdmission_unchecked_pre_fix
    soak_disk/MC_BenchmarkDiskAdmission
    soak_disk/MC_BenchmarkDiskAdmission_unchecked_pre_fix
    soak_disk/MC_BenchmarkDiskMonitor
    soak_disk/MC_BenchmarkDiskMonitor_late_pre_fix
    soak_disk/MC_BenchmarkCancellation
    soak_disk/MC_BenchmarkCancellation_unwatched_death_pre_fix
    soak_disk/MC_BenchmarkCancellation_unwatched_breach_pre_fix
    soak_disk/MC_GuardianProgress
    soak_disk/MC_GuardianProgress_alive_only_pre_fix
    soak_disk/MC_GuardianProgressAdmission
    soak_disk/MC_GuardianProgressAdmission_unchecked_pre_fix
    soak_disk/MC_DiskHygieneDeadline
    soak_disk/MC_DiskHygieneDeadline_unbounded_pre_fix
    soak_disk/MC_DiskSettingsAdmission
    soak_disk/MC_DiskSettingsAdmission_unchecked_pre_fix
    soak_disk/MC_CleanupOwnership
    soak_disk/MC_CleanupOwnership_age_only_pre_fix
    soak_disk/MC_CleanupOutcome
    soak_disk/MC_CleanupOutcome_ignore_errors_pre_fix
    soak_disk/MC_DockerCleanupOwnership
    soak_disk/MC_DockerCleanupOwnership_global_prune_pre_fix
    soak_disk/MC_DockerExitStop
    soak_disk/MC_DockerExitStop_client_only_pre_fix
    soak_disk/MC_IterationCrashRecovery
    soak_disk/MC_IterationCrashRecovery_unrecorded_pre_fix
    soak_disk/MC_DockerStopOwnership
    soak_disk/MC_DockerStopOwnership_name_only_pre_fix
    soak_disk/MC_DockerStopFailure
    soak_disk/MC_DockerStopFailure_ignored_pre_fix
    soak_disk/MC_HostStopOwnership
    soak_disk/MC_HostStopOwnership_pattern_only_pre_fix
    soak_disk/MC_HostStopOwnership_memory_pattern_pre_fix
    soak_disk/MC_HostOomOwnership
    soak_disk/MC_HostOomOwnership_pattern_only_pre_fix
    carrier_index/MC_CarrierIndex_dag_first_pre_fix
    carrier_index/MC_CarrierIndex_read_failure_pre_fix
)
nightly_configs=(
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
    "${pr_configs[@]}"
)
exhaustive_configs=(
    slashing/MC_EquivocationDetector
    slashing/MC_EquivocationDetectorEager_3v
    slashing/MC_EquivocationDetector_safety
)

for scenario in pull_request push schedule dispatch exhaustive failure; do
    event="$scenario"
    exhaustive=0
    fail=0
    expected=("${pr_configs[@]}")
    case "$scenario" in
    schedule) expected=("${nightly_configs[@]}") ;;
    dispatch)
        event=workflow_dispatch
        expected=("${nightly_configs[@]}")
        ;;
    exhaustive)
        event=workflow_dispatch
        exhaustive=1
        expected=("${nightly_configs[@]}" "${exhaustive_configs[@]}")
        ;;
    failure)
        event=pull_request
        fail=1
        ;;
    esac
    status=0
    (cd "$WORK/repo" &&
        PATH="$WORK/bin:$PATH" TLA_TOOLS_JAR="$WORK/no-jar" \
            GITHUB_EVENT_NAME="$event" RUN_EXHAUSTIVE_TLA="$exhaustive" TEST_TLC_FAIL="$fail" \
            bash -euo pipefail "$WORK/workflow-run.sh") >"$WORK/result.log" 2>&1 || status=$?
    if ((fail == 1)); then
        if ((status == 0)); then
            printf 'FAIL: A violated baseline did not fail the pull-request gate.\n' >&2
            exit 1
        fi
    elif ((status != 0)); then
        printf 'FAIL: The formal workflow failed for %s.\n' "$scenario" >&2
        exit 1
    fi
    printf '%s\n' "${expected[@]}" | sort >"$WORK/expected"
    awk '$1 == "CHECK" { print $2 }' "$WORK/result.log" | sort >"$WORK/actual"
    if ! diff -u "$WORK/expected" "$WORK/actual"; then
        printf 'FAIL: The formal configuration inventory changed for %s.\n' "$scenario" >&2
        exit 1
    fi
    if [[ "$event" == pull_request || "$event" == push ]]; then
        if grep '^CHECK' "$WORK/result.log" | grep -vq 'cap 2m)'; then
            printf 'FAIL: A pull-request configuration exceeded the declared two-minute cap.\n' >&2
            exit 1
        fi
    fi
done
printf 'PASS: Pull requests run the bounded formal baseline and preserve nightly verification.\n'
