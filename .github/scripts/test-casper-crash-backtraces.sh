#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
TOOL="$ROOT/.github/scripts/casper-crash-backtraces.sh"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

fail() {
	printf 'FAIL: %s\n' "$*" >&2
	exit 1
}

mkdir -p "$TMP/bin"
cat >"$TMP/bin/sudo" <<'SH'
#!/usr/bin/env bash
printf '%s\n' "$*" >>"$SUDO_LOG"
if [ "$1" = apt-get ]; then exit 1; fi
SH
cat >"$TMP/bin/stub-gdb" <<'SH'
#!/usr/bin/env bash
printf '%s\n' "$*" >>"$GDB_LOG"
printf 'Thread 1 frame\n'
exit "${GDB_EXIT:-0}"
SH
chmod +x "$TMP/bin/sudo" "$TMP/bin/stub-gdb"

export PATH="$TMP/bin:$PATH"
export SUDO_LOG="$TMP/sudo.log" GDB_LOG="$TMP/gdb.log"
export CRASH_CORE_DIR="$TMP/cores" CRASH_LOG_DIR="$TMP/logs" CRASH_GDB=stub-gdb
touch "$SUDO_LOG" "$GDB_LOG"

bash "$TOOL" report >/dev/null || fail "report must pass when the core directory is missing"
[ ! -s "$GDB_LOG" ] || fail "report must not run gdb without a core file"
[ ! -d "$CRASH_LOG_DIR" ] || fail "report must not create the log directory without a core file"

bash "$TOOL" setup
[ -d "$CRASH_CORE_DIR" ] || fail "setup must create the core directory"
grep -qF "sysctl -q -w kernel.core_pattern=$CRASH_CORE_DIR/core.%p.%E" "$SUDO_LOG" \
	|| fail "setup must set the core pattern with the process and the executable"

core="$CRASH_CORE_DIR/core.123.!work!target!release!deps!mod-abc"
touch "$core"
output="$(bash "$TOOL" report)" || fail "report must pass with a core file"
grep -qF "/work/target/release/deps/mod-abc" "$GDB_LOG" || fail "report must decode the executable path"
grep -qF "thread apply all bt" "$GDB_LOG" || fail "report must ask for every thread"
grep -qF "$core" "$GDB_LOG" || fail "report must pass the core file to gdb"
grep -qF "Thread 1 frame" <<<"$output" || fail "report must print the backtrace"
grep -qF "Thread 1 frame" "$CRASH_LOG_DIR/crash-backtrace-123.txt" || fail "report must write the backtrace to the log directory"
[ ! -e "$core" ] || fail "report must delete the core file"

touch "$core"
GDB_EXIT=1 bash "$TOOL" report >/dev/null || fail "report must pass when gdb fails"
[ ! -e "$core" ] || fail "report must delete the core file when gdb fails"

touch "$core"
output="$(CRASH_GDB=missing-gdb bash "$TOOL" report)" || fail "report must pass when gdb cannot be installed"
grep -qF "::warning::gdb is not available" <<<"$output" || fail "report must warn when gdb is not available"
[ ! -e "$core" ] || fail "report must delete the core file when gdb is not available"

if bash "$TOOL" unknown >/dev/null 2>&1; then
	fail "an unknown mode must fail"
fi

echo "PASS: the crash backtrace script sets the core pattern, reports each core file, deletes it, and never fails the job."
