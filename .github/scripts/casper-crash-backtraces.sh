#!/usr/bin/env bash
# Report a native backtrace for each crashed casper test process (issue #911).
#
# `setup` sends core files to a directory, with the executable path in the file
# name. `report` prints a backtrace of every thread of each core file and writes
# it to the test log directory. Then it deletes the core file, because a core
# file can be several GB. `report` always exits with 0, so the test result stays
# the failure signal of the job.
set -euo pipefail

core_dir="${CRASH_CORE_DIR:-/tmp/cores}"
log_dir="${CRASH_LOG_DIR:-/tmp/test-logs}"
gdb_bin="${CRASH_GDB:-gdb}"

setup() {
	mkdir -p "$core_dir"
	sudo sysctl -q -w "kernel.core_pattern=$core_dir/core.%p.%E"
}

install_gdb() {
	command -v "$gdb_bin" >/dev/null && return 0
	sudo apt-get update -qq >/dev/null 2>&1 || true
	sudo apt-get install -y -qq --no-install-recommends gdb >/dev/null 2>&1 || true
	command -v "$gdb_bin" >/dev/null
}

report() {
	shopt -s nullglob
	local cores=("$core_dir"/core.*)
	[ "${#cores[@]}" -gt 0 ] || return 0
	mkdir -p "$log_dir"
	if ! install_gdb; then
		printf '::warning::gdb is not available, so %s core file(s) have no backtrace\n' "${#cores[@]}"
		rm -f "${cores[@]}"
		return 0
	fi
	local core name rest pid exe
	for core in "${cores[@]}"; do
		name="${core##*/}"
		rest="${name#core.}"
		pid="${rest%%.*}"
		exe="${rest#*.}"
		exe="${exe//!//}"
		printf '=== Backtrace of %s, process %s ===\n' "$exe" "$pid"
		"$gdb_bin" -batch -nx -ex 'thread apply all bt' "$exe" "$core" 2>&1 \
			| tee "$log_dir/crash-backtrace-$pid.txt" || true
		rm -f "$core"
	done
}

case "${1:-}" in
setup) setup ;;
report) report ;;
*)
	echo "usage: casper-crash-backtraces.sh setup|report" >&2
	exit 2
	;;
esac
