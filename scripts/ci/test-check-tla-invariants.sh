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
    carrier_index/MC_CarrierIndex_read_failure_pre_fix soak_disk/MC_SoakDisk_floor_only_pre_fix; do
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
