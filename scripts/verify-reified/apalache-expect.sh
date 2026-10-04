#!/usr/bin/env bash
# apalache-expect.sh: run one Apalache inductive check and require its outcome.
#
# Usage:
#   apalache-expect.sh <module.tla> <config.cfg> <length> NoError
#   apalache-expect.sh <module.tla> <config.cfg> <length> Error <marker>
#
# NoError passes when Apalache exits 0 and reports "The outcome is: NoError"
# and "EXITCODE: OK".
#
# Error passes when Apalache exits non-zero, reports "The outcome is: Error"
# and a violated state invariant, and the InvariantViolation section of its
# violation1.tla contains <marker>. The marker names the expected conjunct, so
# a negative control fails if the invariant breaks in a different conjunct.
#
# Work files go under ${RRS_SCRATCH:-<repo>/target/verification/reified-rspaces}.
# A scratch root under /tmp is refused unless RRS_SANDBOX=1 (the pgmcp sandbox
# gives each job a private /tmp).
#
# Exit codes: 0 expected outcome, 1 other outcome, 2 usage or setup error.

set -u

usage() {
  echo "usage: $0 <module.tla> <config.cfg> <length> NoError|Error [marker]" >&2
  exit 2
}

[[ $# -ge 4 ]] || usage
module_path=$1
config=$2
length=$3
expected=$4
marker=${5:-}

case "$expected" in
  NoError) [[ $# -eq 4 ]] || usage ;;
  Error) [[ $# -eq 5 && -n "$marker" ]] || usage ;;
  *) usage ;;
esac
[[ "$length" =~ ^[0-9]+$ ]] || usage

repo_root="$(cd "$(dirname "$0")/../.." && pwd)"
module_dir="$(cd "$(dirname "$module_path")" && pwd)"
module_file="$(basename "$module_path")"
if [[ ! -f "$module_dir/$module_file" || ! -f "$module_dir/$config" ]]; then
  echo "apalache-expect: FAIL: $module_path or $config is missing" >&2
  exit 2
fi
if ! command -v apalache-mc >/dev/null 2>&1; then
  echo "apalache-expect: FAIL: apalache-mc is not installed" >&2
  exit 2
fi

scratch_root="${RRS_SCRATCH:-$repo_root/target/verification/reified-rspaces}"
if [[ "$scratch_root" == /tmp* && "${RRS_SANDBOX:-0}" != "1" ]]; then
  echo "apalache-expect: FAIL: refusing a /tmp scratch root outside the sandbox" >&2
  exit 2
fi
work="$scratch_root/apalache-expect/${module_file%.tla}-${config%.cfg}-$length"
rm -rf -- "$work"
mkdir -p "$work/tla" "$work/out" "$work/tmp" || exit 2
cp -- "$module_dir"/*.tla "$module_dir/$config" "$work/tla/" || exit 2

log="$work/apalache.log"
(cd "$work/tla" && TMPDIR="$work/tmp" JVM_ARGS="${JVM_ARGS:--Xmx4g -Djava.io.tmpdir=$work/tmp}" \
   apalache-mc check --out-dir="$work/out" --config="$config" --length="$length" "$module_file") \
   > "$log" 2>&1
rc=$?

if [[ "$expected" == "NoError" ]]; then
  if [[ $rc -eq 0 ]] && grep -q 'The outcome is: NoError' "$log" && grep -q 'EXITCODE: OK' "$log"; then
    echo "apalache-expect: PASS $module_file $config --length=$length: NoError"
    exit 0
  fi
  echo "apalache-expect: FAIL $module_file $config --length=$length: expected NoError, exit $rc"
  grep -E 'violated|outcome|EXITCODE|rror' "$log" | head -n 5
  exit 1
fi

violation="$(find "$work/out" -name violation1.tla -print -quit 2>/dev/null)"
if [[ $rc -ne 0 ]] && grep -q 'The outcome is: Error' "$log" \
   && grep -qE 'state invariant [0-9]+ violated' "$log" \
   && [[ -n "$violation" ]] \
   && awk '/^InvariantViolation/{p=1} p' "$violation" | grep -qF -- "$marker"; then
  echo "apalache-expect: PASS $module_file $config --length=$length: the inductive invariant breaks in the expected conjunct ($marker)"
  exit 0
fi
echo "apalache-expect: FAIL $module_file $config --length=$length: expected a violation that mentions '$marker', exit $rc"
grep -E 'violated|outcome|EXITCODE' "$log" | head -n 5
exit 1
