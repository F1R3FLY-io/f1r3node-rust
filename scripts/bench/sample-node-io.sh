#!/usr/bin/env bash
# Diagnostic sampler for one soak iteration (issue #24, subprocess checkpoint
# parity). It records why the LMDB write stages of subprocess nodes are slower
# than those of Docker nodes. It never fails the iteration: every read that
# fails is recorded as unavailable.
#
#   sample-node-io.sh ITERATION_DIR
#
# Output in ITERATION_DIR/io-diagnostics/:
#   io.tsv          every SOAK_IO_INTERVAL_SECONDS: /proc/<pid>/io and the page
#                   faults of each node process
#   nodes.txt       once per node: whether it ran at sampler start, data
#                   directory, its mount, cgroup, and the libc, libssl, and
#                   jemalloc objects that the process mapped
#   host.txt        once: the vm.dirty_* settings
#   strace-<role>.txt  once, SOAK_IO_STRACE_DELAY_SECONDS after the
#                   "--- Phase: sustained ---" marker (or SOAK_IO_STRACE_AFTER_SECONDS
#                   after the start without it): syscall summary of the node whose
#                   role, or last dot-separated part of the role, equals
#                   SOAK_IO_STRACE_ROLE, for SOAK_IO_STRACE_SECONDS. It runs in the
#                   background, so io.tsv sampling continues. strace -f slows the
#                   traced node, so compare io.tsv rows of other nodes for timing.
#                   A node that ran at sampler start is never the target: the
#                   iteration starts its shard after the sampler, and a benchmark
#                   shard that runs across iterations has the same roles
#                   (run 38995239992, iterations 84 to 93).
#
# Test seams: PROC_ROOT, SOAK_IO_SUDO, SOAK_IO_DOCKER, SOAK_IO_STRACE.
set -uo pipefail

ITERATION_DIR="${1:?usage: sample-node-io.sh ITERATION_DIR}"
OUT="$ITERATION_DIR/io-diagnostics"
PROC_ROOT="${PROC_ROOT:-/proc}"
INTERVAL="${SOAK_IO_INTERVAL_SECONDS:-30}"
STRACE_ROLE="${SOAK_IO_STRACE_ROLE:-validator1}"
STRACE_SECONDS="${SOAK_IO_STRACE_SECONDS:-30}"
STRACE_AFTER="${SOAK_IO_STRACE_AFTER_SECONDS:-900}"
STRACE_DELAY="${SOAK_IO_STRACE_DELAY_SECONDS:-60}"
SUDO="${SOAK_IO_SUDO-sudo -n}"
DOCKER="${SOAK_IO_DOCKER:-docker}"
STRACE="${SOAK_IO_STRACE:-strace}"
PYTEST_LOG="$ITERATION_DIR/pytest.log"

mkdir -p "$OUT" || exit 0
STRACE_PID=""
ROLE_CACHE="$OUT/.roles"
: >"$ROLE_CACHE"
stop() {
	if [ -n "$STRACE_PID" ]; then
		pkill -TERM -P "$STRACE_PID" 2>/dev/null || true
		kill "$STRACE_PID" 2>/dev/null || true
	fi
	exit 0
}
trap stop TERM INT

read_proc() {
	cat "$1" 2>/dev/null || { [ -n "$SUDO" ] && $SUDO cat "$1" 2>/dev/null; }
}

# Prints "pid<TAB>role<TAB>data_dir" for each node process: a process whose
# arguments contain "run", started from the image binary or with --data-dir.
discover_nodes() {
	local dir pid args arg prev data_dir role is_node cid
	for dir in "$PROC_ROOT"/[0-9]*; do
		pid="${dir##*/}"
		args="$(read_proc "$dir/cmdline" | tr '\0' '\n')" || continue
		[ -n "$args" ] || continue
		grep -qx run <<<"$args" || continue
		is_node=0 data_dir="" prev=""
		case "$(head -1 <<<"$args")" in */bin/node | */rnode | */rnode.bin | node) is_node=1 ;; esac
		while IFS= read -r arg; do
			[ "$prev" = --data-dir ] && data_dir="$arg"
			case "$arg" in --data-dir=*) data_dir="${arg#--data-dir=}" ;; esac
			prev="$arg"
		done <<<"$args"
		[ -n "$data_dir" ] && is_node=1
		[ "$is_node" -eq 1 ] || continue
		if [ -n "$data_dir" ]; then
			role="${data_dir%/}"
			role="${role##*/}"
		else
			data_dir=/var/lib/rnode
			role="$(awk -F'\t' -v p="$pid" '$1 == p { print $2; exit }' "$ROLE_CACHE" 2>/dev/null)"
			if [ -z "$role" ]; then
				cid="$(read_proc "$dir/cgroup" | grep -oE '[0-9a-f]{64}' | head -1)"
				[ -n "$cid" ] && role="$($DOCKER inspect --format '{{.Name}}' "$cid" 2>/dev/null)"
				role="${role#/}"
				[ -n "$role" ] || role="pid-$pid"
				printf '%s\t%s\n' "$pid" "$role" >>"$ROLE_CACHE"
			fi
		fi
		printf '%s\t%s\t%s\n' "$pid" "$role" "$data_dir"
	done
}

io_field() { awk -v k="$1:" '$1 == k { print $2; found = 1 } END { if (!found) print "NA" }' <<<"$2"; }

# Fields 10 (minflt) and 12 (majflt) of /proc/<pid>/stat, after the command.
fault_fields() {
	local stat rest
	stat="$(read_proc "$PROC_ROOT/$1/stat")" || { printf 'NA\tNA'; return; }
	rest="${stat##*) }"
	awk '{ printf "%s\t%s", $8, $10 }' <<<"$rest"
}

sample_io() {
	local now pid role data_dir io
	now="$(date +%s)"
	while IFS=$'\t' read -r pid role data_dir; do
		io="$(read_proc "$PROC_ROOT/$pid/io")" || io=""
		printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' "$now" "$pid" "$role" \
			"$(io_field rchar "$io")" "$(io_field wchar "$io")" "$(io_field syscr "$io")" "$(io_field syscw "$io")" \
			"$(io_field read_bytes "$io")" "$(io_field write_bytes "$io")" "$(io_field cancelled_write_bytes "$io")" \
			"$(fault_fields "$pid")" >>"$OUT/io.tsv"
	done
}

# The mount whose mount point is the longest prefix of the data directory,
# in the mount namespace of the process.
data_dir_mount() {
	read_proc "$PROC_ROOT/$1/mountinfo" | awk -v d="$2" '
		{ mp = $5; if ((d == mp || index(d, mp == "/" ? "/" : mp "/") == 1) && length(mp) >= best) { best = length(mp); line = $0 } }
		END { if (line != "") print line; else print "unavailable" }'
}

describe_node() {
	local pid="$1" role="$2" data_dir="$3"
	{
		printf '== %s (pid %s)\n' "$role" "$pid"
		case "$PREEXISTING" in *" $pid "*) printf 'running at sampler start: yes\n' ;; *) printf 'running at sampler start: no\n' ;; esac
		printf 'data_dir: %s\n' "$data_dir"
		printf 'mount: %s\n' "$(data_dir_mount "$pid" "$data_dir")"
		printf 'cgroup: %s\n' "$(read_proc "$PROC_ROOT/$pid/cgroup" | tr '\n' ' ')"
		printf 'libraries:\n'
		read_proc "$PROC_ROOT/$pid/maps" | awk '$6 ~ /(libc[.-]|libssl|libcrypto|libjemalloc|ld-linux)/ { print "  " $6 }' | sort -u
	} >>"$OUT/nodes.txt"
}

describe_host() {
	local f
	for f in "$PROC_ROOT"/sys/vm/dirty_*; do
		[ -e "$f" ] && printf '%s = %s\n' "${f##*/}" "$(read_proc "$f")"
	done >"$OUT/host.txt"
}

run_strace() {
	local pid="$1" trigger="$2" target="$OUT/strace-$STRACE_ROLE.txt" cmd
	cmd=(timeout --preserve-status -s INT "$STRACE_SECONDS" "$STRACE" -c -f -p "$pid" -e trace=fsync,fdatasync,msync,pwrite64,write,mmap,munmap,madvise)
	printf 'role: %s\npid: %s\ntrigger: %s\nseconds: %s\nnote: strace -f slows the traced node during the sample.\n\n' \
		"$STRACE_ROLE" "$pid" "$trigger" "$STRACE_SECONDS" >"$target"
	if ! command -v "$STRACE" >/dev/null 2>&1; then
		printf 'strace unavailable\n' >>"$target"
		return
	fi
	if "${cmd[@]}" >"$target.direct" 2>&1; then
		cat "$target.direct" >>"$target"
	elif [ -n "$SUDO" ] && $SUDO "${cmd[@]}" >"$target.sudo" 2>&1; then
		cat "$target.sudo" >>"$target"
	else
		printf 'strace failed:\n' >>"$target"
		cat "$target.direct" "$target.sudo" >>"$target" 2>/dev/null
	fi
	rm -f "$target.direct" "$target.sudo"
}

printf 'ts\tpid\trole\trchar\twchar\tsyscr\tsyscw\tread_bytes\twrite_bytes\tcancelled_write_bytes\tminflt\tmajflt\n' >"$OUT/io.tsv"
describe_host
PREEXISTING=" $(discover_nodes | cut -f1 | tr '\n' ' ')"
started="$(date +%s)"
described=" "
straced=0
sustained_at=""
while :; do
	nodes="$(discover_nodes)"
	if [ -n "$nodes" ]; then
		sample_io <<<"$nodes"
		while IFS=$'\t' read -r pid role data_dir; do
			case "$described" in *" $pid "*) continue ;; esac
			describe_node "$pid" "$role" "$data_dir"
			described="$described$pid "
		done <<<"$nodes"
		if [ "$straced" -eq 0 ]; then
			now="$(date +%s)"
			if [ -z "$sustained_at" ] && grep -q '^--- Phase: sustained ---' "$PYTEST_LOG" 2>/dev/null; then sustained_at="$now"; fi
			if { [ -n "$sustained_at" ] && [ "$((now - sustained_at))" -ge "$STRACE_DELAY" ]; } || [ "$((now - started))" -ge "$STRACE_AFTER" ]; then
				pid="$(awk -F'\t' -v r="$STRACE_ROLE" -v skip="$PREEXISTING" '
					index(skip, " " $1 " ") { next }
					{ n = split($2, part, "."); if ($2 == r || part[n] == r) { print $1; exit } }' <<<"$nodes")"
				if [ -n "$pid" ]; then
					if [ -n "$sustained_at" ]; then trigger="sustained phase marker plus ${STRACE_DELAY}s"; else trigger="fallback after ${STRACE_AFTER}s without the marker"; fi
					run_strace "$pid" "$trigger" &
					STRACE_PID=$!
					straced=1
				fi
			fi
		fi
	fi
	sleep "$INTERVAL" &
	wait $! 2>/dev/null || exit 0
done
