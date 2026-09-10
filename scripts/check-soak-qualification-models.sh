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
qualification_evidence=$(mktemp -d "$PWD/target/verification/soak-qualification/models.XXXXXX")
mkdir -p "$qualification_evidence/tmp" "$qualification_evidence/rocq"
export TMPDIR="$qualification_evidence/tmp"
printf 'Evidence: %s\n' "$qualification_evidence"
sha256sum scripts/check-soak-qualification-models.sh \
    formal/tlaplus/soak_qualification/SoakQualification.tla \
    formal/tlaplus/soak_qualification/Safe.cfg \
    formal/tlaplus/soak_qualification/ElapsedOnlyUnsafe.cfg \
    formal/rocq/soak_qualification/SoakQualification.v \
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
coqc -q -o "$qualification_evidence/rocq/SoakQualification.vo" \
    formal/rocq/soak_qualification/SoakQualification.v \
    > "$qualification_evidence/rocq.log" 2>&1
coqchk -silent -Q "$qualification_evidence/rocq" '' SoakQualification \
    > "$qualification_evidence/kernel.log" 2>&1
[[ $(rg -c '^Closed under the global context$' "$qualification_evidence/rocq.log") == 9 ]]
for configuration in Safe ElapsedOnlyUnsafe; do
    set +e
    java -Xmx512m -XX:+UseSerialGC -Djava.io.tmpdir="$TMPDIR" \
        -cp /usr/share/java/tla2tools.jar tlc2.TLC -workers 1 \
        -metadir "$qualification_evidence/states-$configuration" \
        -config "formal/tlaplus/soak_qualification/$configuration.cfg" \
        formal/tlaplus/soak_qualification/SoakQualification.tla \
        > "$qualification_evidence/$configuration.log" 2>&1
    result=$?
    set -e
    tail -n 12 "$qualification_evidence/$configuration.log"
    printf '%s\n' "$result" > "$qualification_evidence/$configuration.exit"
    if [[ "$configuration" == Safe ]]; then
        [[ "$result" == 0 ]]
        rg -q -F 'Model checking completed. No error has been found.' "$qualification_evidence/$configuration.log"
    else
        [[ "$result" == 12 ]]
        rg -q -F 'Invariant QualifiedImpliesContract is violated' "$qualification_evidence/$configuration.log"
    fi
done
