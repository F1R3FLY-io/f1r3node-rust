#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
qualification_cgroup=$(awk -F: '$1 == "0" { print $3 }' /proc/self/cgroup)
qualification_memory=$(<"/sys/fs/cgroup$qualification_cgroup/memory.max")
qualification_swap=$(<"/sys/fs/cgroup$qualification_cgroup/memory.swap.max")
if [[ ! "$qualification_memory" =~ ^[1-9][0-9]*$ ]] \
    || (( qualification_memory > 2147483648 )) || [[ "$qualification_swap" != 0 ]]; then
    echo 'Use a systemd scope with MemoryMax at most 2 GiB and MemorySwapMax=0.' >&2
    exit 2
fi
mkdir -p target/verification/soak-qualification
qualification_evidence=$(mktemp -d "$PWD/target/verification/soak-qualification/verifier.XXXXXX")
mkdir -p "$qualification_evidence/tmp"
export TMPDIR="$qualification_evidence/tmp"
export PYTHONDONTWRITEBYTECODE=1
printf 'Evidence: %s\n' "$qualification_evidence"
sha256sum scripts/test-merge-recovery-soak-qualification.sh \
    scripts/soak/qualification.py scripts/soak/test_qualification.py \
    scripts/soak/collect_qualification.py scripts/soak/test_collector.py \
    scripts/soak/test_workflow.py scripts/run-merge-recovery-soak.sh \
    scripts/soak/test_driver_qualification.py \
    scripts/bench/test-run-merge-recovery-soak.sh \
    scripts/bench/write-soak-summary.sh scripts/bench/soak-metrics.json \
    .github/scripts/check-workflow-invariants.sh \
    .github/workflows/merge-recovery-soak.yml .github/actions/soak-segment/action.yml \
    > "$qualification_evidence/inputs.sha256"
qualification_finish() {
    local result=$?
    trap - EXIT
    sha256sum -c "$qualification_evidence/inputs.sha256" \
        > "$qualification_evidence/inputs-check.log" || result=1
    printf '%s\n' "$result" > "$qualification_evidence/gate.exit"
    printf 'Gate exit: %s\n' "$result"
    exit "$result"
}
trap qualification_finish EXIT
python3 -m unittest discover -s scripts/soak -p 'test_*.py' -v > "$qualification_evidence/tests.log" 2>&1
tail -n 30 "$qualification_evidence/tests.log"
timeout 90 bash scripts/bench/test-run-merge-recovery-soak.sh \
    > "$qualification_evidence/legacy-driver.log" 2>&1
ruff check --no-cache scripts/soak \
    > "$qualification_evidence/ruff.log" 2>&1
ruff format --check scripts/soak \
    > "$qualification_evidence/format.log" 2>&1
shellcheck scripts/check-soak-qualification-models.sh scripts/test-merge-recovery-soak-qualification.sh \
    > "$qualification_evidence/shellcheck.log" 2>&1
bash -n scripts/run-merge-recovery-soak.sh
bash .github/scripts/check-workflow-invariants.sh > "$qualification_evidence/workflow-invariants.log" 2>&1
