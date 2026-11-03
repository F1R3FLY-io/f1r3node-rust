#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
source_runtime="$(bash "$ROOT/scripts/ci/install-cbc-runtime.sh" --check)"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
fixture="$TMP/project"
mkdir -p "$fixture/scripts/ci" "$TMP/bin"
git init -q "$fixture"
cp "$ROOT/scripts/cbc.sh" "$ROOT/scripts/cbc-runtime.lock.json" "$fixture/scripts/"
cp "$ROOT/scripts/ci/install-cbc-runtime.sh" "$fixture/scripts/ci/"
printf 'source\n' >"$fixture/source.txt"
printf '/source.txt cbc=mandatory cbc-weight=high\n' >"$fixture/.gitattributes"
export CBC_RUNTIME_TEST_SOURCE="$source_runtime"
export CBC_RUNTIME_TEST_LOG="$TMP/downloads"
export CBC_RUNTIME_TEST_MARKER="$TMP/executed"
export CBC_RUNTIME_CACHE="$TMP/cache"
cat >"$TMP/bin/curl" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
output=""
url=""
while [ "$#" -gt 0 ]; do
    case "$1" in
        --output) output="$2"; shift 2 ;;
        *) url="$1"; shift ;;
    esac
done
printf '%s\n' "$url" >>"$CBC_RUNTIME_TEST_LOG"
[ "${CBC_RUNTIME_TEST_FAIL:-0}" -eq 0 ] || exit 22
path="${url#*/AItools/scripts/}"
cp "$CBC_RUNTIME_TEST_SOURCE/$path" "$output"
[ "${CBC_RUNTIME_TEST_CORRUPT:-0}" -eq 0 ] || printf '\nchanged\n' >>"$output"
SH
chmod +x "$TMP/bin/curl"
export PATH="$TMP/bin:$PATH"
installer="$fixture/scripts/ci/install-cbc-runtime.sh"
launcher="$fixture/scripts/cbc.sh"
failure() {
    local status=0
    "$@" >"$TMP/failure.log" 2>&1 || status=$?
    [ "$status" -eq 4 ] || { cat "$TMP/failure.log"; printf 'Expected a closed runtime failure, got %s.\n' "$status" >&2; exit 1; }
}
failure bash "$launcher" attributes evidence-path source.txt
[ ! -e "$CBC_RUNTIME_TEST_LOG" ]
installed="$(bash "$installer")"
[ "$(wc -l <"$CBC_RUNTIME_TEST_LOG" | tr -d ' ')" -eq "$(jq '.sha256|length' "$ROOT/scripts/cbc-runtime.lock.json")" ]
before="$(cat "$CBC_RUNTIME_TEST_LOG")"
[ "$(bash "$installer")" = "$installed" ]
[ "$(cat "$CBC_RUNTIME_TEST_LOG")" = "$before" ]
[ "$(bash "$launcher" attributes evidence-path source.txt)" = docs/cbc-evidence/source-txt.md ]
chmod u+w "$installed/cbc.sh"
printf '\ntouch "$CBC_RUNTIME_TEST_MARKER"\n' >>"$installed/cbc.sh"
failure bash "$launcher" attributes evidence-path source.txt
[ ! -e "$CBC_RUNTIME_TEST_MARKER" ]
[ "$(cat "$CBC_RUNTIME_TEST_LOG")" = "$before" ]
cp "$source_runtime/cbc.sh" "$installed/cbc.sh"
chmod a-w "$installed/cbc.sh"
rm -f "$installed/cbc.sh"
ln -s "$source_runtime/cbc.sh" "$installed/cbc.sh"
failure bash "$launcher" attributes evidence-path source.txt
rm -f "$installed/cbc.sh"
cp "$source_runtime/cbc.sh" "$installed/cbc.sh"
printf 'exit 0\n' >"$installed/lib/cbc-adapters/unlocked.sh"
failure bash "$launcher" attributes evidence-path source.txt
rm -f "$installed/lib/cbc-adapters/unlocked.sh"
jq '.revision="dev"' "$ROOT/scripts/cbc-runtime.lock.json" >"$fixture/scripts/cbc-runtime.lock.json"
failure bash "$installer"
jq '.sha256["../escape.sh"]="0000000000000000000000000000000000000000000000000000000000000000"' \
    "$ROOT/scripts/cbc-runtime.lock.json" >"$fixture/scripts/cbc-runtime.lock.json"
failure bash "$installer"
cp "$ROOT/scripts/cbc-runtime.lock.json" "$fixture/scripts/cbc-runtime.lock.json"
failure env CBC_RUNTIME_CACHE="$TMP/corrupt" CBC_RUNTIME_TEST_CORRUPT=1 bash "$installer"
[ -z "$(find "$TMP/corrupt" -mindepth 1 -print -quit)" ]
failure env CBC_RUNTIME_CACHE="$TMP/failed" CBC_RUNTIME_TEST_FAIL=1 bash "$installer"
[ -z "$(find "$TMP/failed" -mindepth 1 -print -quit)" ]
failure env CBC_RUNTIME_CACHE="$fixture/cache" bash "$installer"
env CBC_RUNTIME_CACHE="$TMP/concurrent" bash "$installer" >"$TMP/install-1.log" 2>&1 &
first=$!
env CBC_RUNTIME_CACHE="$TMP/concurrent" bash "$installer" >"$TMP/install-2.log" 2>&1 &
second=$!
wait "$first" || { cat "$TMP/install-1.log"; exit 1; }
wait "$second" || { cat "$TMP/install-2.log"; exit 1; }
cmp "$TMP/install-1.log" "$TMP/install-2.log"
env CBC_RUNTIME_CACHE="$TMP/concurrent" bash "$installer" --check >/dev/null
jq '.sha256["cbc.sh"]="0000000000000000000000000000000000000000000000000000000000000000"' \
    "$ROOT/scripts/cbc-runtime.lock.json" >"$fixture/scripts/cbc-runtime.lock.json"
failure env CBC_RUNTIME_CACHE="$TMP/wrong-pin" bash "$installer"
[ -z "$(find "$TMP/wrong-pin" -mindepth 1 -print -quit)" ]
printf 'CbC runtime dependency tests passed (14 checks).\n'
