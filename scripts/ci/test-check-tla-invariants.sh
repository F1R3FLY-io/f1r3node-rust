#!/usr/bin/env bash
# Tests scripts/ci/check-tla-invariants.sh with a verifier-process fixture in
# place of TLC. The registered negative controls are read from the gate itself
# so this test never carries a second copy of that list.
#
#  1. Classification: for every registered control the gate accepts only the
#     exact expected violation (exit 12 plus the invariant message) and rejects
#     clean, other-invariant, tool-error, wrong-exit, timeout, and missing-config
#     outcomes.
#  2. Routing: the slashing-tests workflow sends pull requests and pushes
#     through the bounded --soak-pr tier and schedule/dispatch through the full
#     list; the PR tier is a strict subset that still carries every control and
#     runs under the 2m cap; a baseline violation fails the PR tier.
#  3. Registration: a pre-fix config beside a registered positive in a
#     registered area (REGISTERED_CONTROL_AREAS) must appear in the registry;
#     the same file outside those areas stays a manual control.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
GATE="$WORK/repo/scripts/ci/check-tla-invariants.sh"
mkdir -p "$WORK/bin" "$WORK/repo/scripts/ci" "$WORK/repo/formal"
cp "$ROOT/scripts/ci/check-tla-invariants.sh" "$ROOT/scripts/ci/check-formal-invariants.sh" "$WORK/repo/scripts/ci/"
cp -R "$ROOT/formal/tlaplus" "$WORK/repo/formal/"

fail() {
    printf 'FAIL: %s\n' "$1" >&2
    exit 1
}

# <subdir>/<config>:<invariant> entries of the gate's NEGATIVE_CONTROLS array.
mapfile -t CONTROLS < <(sed -n '/^NEGATIVE_CONTROLS=(/,/^)/p' "$GATE" |
    grep -oE '[A-Za-z0-9_]+/MC_[A-Za-z0-9_]+:[A-Za-z0-9_]+')
((${#CONTROLS[@]})) || fail 'The gate registers no negative controls.'
for check in "${CONTROLS[@]}"; do
    printf '%s.cfg\t%s\n' "${check%%:*}" "${check#*:}" | sed 's#^[^/]*/##'
done >"$WORK/controls.tsv"

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
invariant="$(awk -F '\t' -v c="$config" '$1 == c { print $2 }' "$TEST_TLC_CONTROLS")"
if [[ "$config" == "${TEST_TLC_TARGET:-}" ]]; then
    case "$TEST_TLC_RESULT" in
        clean) printf 'Model checking completed. No error has been found.\n'; exit 0 ;;
        wrong-invariant) invariant=TypeOK ;;
        tool-error) printf 'Error: The configuration could not be parsed.\n'; exit 1 ;;
        wrong-exit) printf 'Error: Invariant %s is violated.\n' "$invariant"; exit 1 ;;
        timeout) exit 124 ;;
        violate) invariant=Baseline ;;
    esac
fi
if [[ -z "$invariant" ]]; then
    printf 'Model checking completed. No error has been found.\n'
    exit 0
fi
printf 'Error: Invariant %s is violated.\n' "$invariant"
printf 'Error: The behavior up to this point is:\n'
exit 12
SH
chmod +x "$WORK/bin/tlc"

run_gate() {
    # $1 = "gate" / "gate-pr" to call the gate directly, or a GitHub event name
    # to run the workflow step's command; remaining args are TEST_TLC_* overrides.
    local mode="$1" command
    shift
    case "$mode" in
        gate) command='bash scripts/ci/check-tla-invariants.sh' ;;
        gate-pr) command='bash scripts/ci/check-tla-invariants.sh --soak-pr' ;;
        *) command="GITHUB_EVENT_NAME=$mode bash -euo pipefail $WORK/workflow-run.sh" ;;
    esac
    (cd "$WORK/repo" &&
        env PATH="$WORK/bin:$PATH" TLA_TOOLS_JAR="$WORK/no-jar" RUN_EXHAUSTIVE_TLA=0 \
            TEST_TLC_CONTROLS="$WORK/controls.tsv" TEST_TLC_TARGET="" TEST_TLC_RESULT="" "$@" \
            bash -c "$command") >"$WORK/run.log" 2>&1
}

checked() { awk '$1 == "CHECK" { print $2 }' "$WORK/run.log" | sort; }

# 1. Classification.
for check in "${CONTROLS[@]}"; do
    target="${check%%:*}"
    for result in clean wrong-invariant tool-error wrong-exit timeout missing expected; do
        config="$WORK/repo/formal/tlaplus/$target.cfg"
        [[ "$result" != missing ]] || mv "$config" "$config.saved"
        status=0
        run_gate gate TEST_TLC_TARGET="${target##*/}.cfg" TEST_TLC_RESULT="$result" || status=$?
        [[ "$result" != missing ]] || mv "$config.saved" "$config"
        if [[ "$result" == expected ]]; then
            ((status == 0)) || fail "The gate rejected the expected violation for $target."
        elif ((status == 0)); then
            fail "The gate accepted $target with result $result."
        fi
    done
done

# 2. Routing. A stdlib-only scan of the workflow file: the checks need the
# trigger keys, the job's scalar keys, and one step's env and run block, and
# the file is two-space YAML, so an indentation walk is enough.
python3 - "$ROOT" "$WORK" <<'PY'
import sys
root, work = sys.argv[1:3]
lines = open(f"{root}/.github/workflows/slashing-tests.yml").read().splitlines()

def indent(line):
    return len(line) - len(line.lstrip(" "))

def content(line):
    return bool(line.strip()) and not line.strip().startswith("#")

def children(start, base):
    out = []
    for line in lines[start + 1:]:
        if not line.strip():
            out.append(line)
            continue
        if indent(line) <= base:
            break
        out.append(line)
    return out

def mapping(block, level):
    keys = {}
    for line in block:
        if content(line) and indent(line) == level and ":" in line and not line.strip().startswith("- "):
            key, _, value = line.strip().partition(":")
            keys[key] = value.strip()
    return keys

def abort(message):
    sys.exit(f"FAIL: {message}")

top = {line[:-1]: i for i, line in enumerate(lines) if line and not line.startswith(" ") and line.endswith(":")}
if "on" not in top or "jobs" not in top:
    abort("The formal workflow has no trigger or jobs block.")
if "pull_request" not in mapping(children(top["on"], 0), 2):
    abort("The formal workflow does not trigger on pull requests.")
job_start = next((i for i, line in enumerate(lines) if line == "  tla-model-check:"), None)
if job_start is None or job_start < top["jobs"]:
    abort("The TLA+ invariant job is missing.")
job_block = children(job_start, 2)
job = mapping(job_block, 4)
if job.get("if") not in (None, "true"):
    abort("The TLA+ invariant job is not enabled for every pull request.")
budget = "${{ (github.event_name == 'schedule' || github.event_name == 'workflow_dispatch') && 240 || 15 }}"
if job.get("timeout-minutes") != budget:
    abort("The formal job must bound PR runs to 15 minutes and keep the nightly budget.")
if "continue-on-error" in job:
    abort("Formal errors must fail the job.")

steps = [offset for offset, line in enumerate(job_block)
         if content(line) and indent(line) == 6 and line.strip().startswith("- ")]
found = None
for number, offset in enumerate(steps):
    stop = steps[number + 1] if number + 1 < len(steps) else len(job_block)
    body = [job_block[offset].replace("- ", "  ", 1)] + job_block[offset + 1:stop]
    keys = mapping(body, 8)
    run_index = next((i for i, line in enumerate(body)
                      if content(line) and indent(line) == 8 and line.strip().startswith("run:")), None)
    if run_index is None:
        continue
    if keys["run"] == "|":
        raw = [line for line in body[run_index + 1:] if not (content(line) and indent(line) <= 8)]
        while raw and not raw[-1].strip():
            raw.pop()
        width = min(indent(line) for line in raw if line.strip())
        run = "\n".join(line[width:] for line in raw) + "\n"
    else:
        run = keys["run"] + "\n"
    if "scripts/ci/check-formal-invariants.sh" not in run:
        continue
    found = (keys, body, run)
    break
if found is None:
    abort("The formal verification command is missing.")
keys, body, run = found
env_index = next((i for i, line in enumerate(body)
                  if content(line) and indent(line) == 8 and line.strip() == "env:"), None)
env = mapping(body[env_index + 1:], 10) if env_index is not None else {}
exhaustive = "${{ (github.event_name == 'workflow_dispatch' && inputs.run_exhaustive) && '1' || '0' }}"
if env.get("RUN_EXHAUSTIVE_TLA") != exhaustive:
    abort("Only manual dispatch can select exhaustive verification.")
if "continue-on-error" in keys or "if" in keys:
    abort("Formal errors must fail the job.")
open(f"{work}/workflow-run.sh", "w").write(run)
PY

run_gate gate || fail 'The full gate failed with the fixture.'
checked >"$WORK/full"
run_gate gate-pr || fail 'The PR-tier gate failed with the fixture.'
checked >"$WORK/pr"
if grep '^CHECK' "$WORK/run.log" | grep -vq 'cap 2m)'; then
    fail 'A PR-tier configuration is not under the two-minute cap.'
fi
[[ -s "$WORK/pr" && -s "$WORK/full" ]] || fail 'A gate tier checked nothing.'
if comm -23 "$WORK/pr" "$WORK/full" | grep -q .; then
    fail 'The PR tier checks a configuration the full tier does not.'
fi
comm -13 "$WORK/pr" "$WORK/full" | grep -q . || fail 'The PR tier is not a strict subset of the full tier.'
for check in "${CONTROLS[@]}"; do
    grep -Fxq "${check%%:*}" "$WORK/pr" || fail "The PR tier omits the negative control ${check%%:*}."
done

for event in pull_request push; do
    run_gate "$event" || fail "The workflow command failed for $event."
    diff -u "$WORK/pr" <(checked) || fail "The workflow ran a different set than the PR tier for $event."
done
for event in schedule workflow_dispatch; do
    run_gate "$event" || fail "The workflow command failed for $event."
    diff -u "$WORK/full" <(checked) || fail "The workflow ran a different set than the full tier for $event."
done
run_gate workflow_dispatch RUN_EXHAUSTIVE_TLA=1 || fail 'The exhaustive dispatch failed with the fixture.'
comm -13 "$WORK/full" <(checked) | grep -q . || fail 'Exhaustive dispatch added no configurations.'

printf '%s\n' "${CONTROLS[@]}" | sed 's/:.*//' | sort >"$WORK/control-configs"
baseline="$(comm -23 "$WORK/pr" "$WORK/control-configs" | head -1)"
[[ -n "$baseline" ]] || fail 'The PR tier has no baseline configuration to violate.'
if run_gate pull_request TEST_TLC_TARGET="${baseline##*/}.cfg" TEST_TLC_RESULT=violate; then
    fail 'A violated baseline did not fail the pull-request gate.'
fi

# 3. Registration.
tla="$WORK/repo/formal/tlaplus"
planted="$tla/soak_disk/MC_SoakDiskAdmission_planted_pre_fix.cfg"
cp "$tla/soak_disk/MC_SoakDiskAdmission_floor_only_pre_fix.cfg" "$planted"
if run_gate gate; then
    fail 'The gate accepted an unregistered pre-fix configuration in a registered area.'
fi
grep -q 'not registered in NEGATIVE_CONTROLS' "$WORK/run.log" ||
    fail 'The gate failed for a reason other than the unregistered control.'
rm -f "$planted"
planted="$tla/replay_liveness/MC_ReplayHotLoop_planted_pre_fix.cfg"
cp "$tla/replay_liveness/MC_ReplayHotLoop_quadratic_pre_fix.cfg" "$planted"
run_gate gate || fail 'A manual control outside the registered areas failed the gate.'
rm -f "$planted"

printf 'PASS: %s negative controls classify exactly; PR/push run the bounded tier and schedule/dispatch run the full tier; unregistered controls are caught.\n' "${#CONTROLS[@]}"
