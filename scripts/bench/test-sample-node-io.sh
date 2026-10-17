#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
TOOL="$ROOT/scripts/bench/sample-node-io.sh"
TMP="$(mktemp -d)"
SAMPLER=""
trap '[ -z "$SAMPLER" ] || kill "$SAMPLER" 2>/dev/null || true; rm -rf "$TMP"' EXIT

fail() {
	printf 'FAIL: %s\n' "$*" >&2
	exit 1
}

PROC="$TMP/proc"
CID=0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef

fake_process() {
	local pid="$1" comm="$2" write_bytes="$3" minflt="$4" majflt="$5"
	mkdir -p "$PROC/$pid"
	printf '%s (%s) S 1 %s %s 0 -1 4194560 %s 0 %s 0 0 0\n' "$pid" "$comm" "$pid" "$pid" "$minflt" "$majflt" >"$PROC/$pid/stat"
	printf 'rchar: 10\nwchar: 20\nsyscr: 3\nsyscw: 4\nread_bytes: 0\nwrite_bytes: %s\ncancelled_write_bytes: 7\n' "$write_bytes" >"$PROC/$pid/io"
}

fake_process 101 node 4096 11 1
printf '/opt/docker/bin/node\0--profile=docker\0run\0' >"$PROC/101/cmdline"
printf '0::/system.slice/docker-%s.scope\n' "$CID" >"$PROC/101/cgroup"
printf '36 25 0:31 /volumes/v1/_data /var/lib/rnode rw,relatime - ext4 /dev/sda1 rw\n24 1 0:30 / / rw - overlay overlay rw\n' >"$PROC/101/mountinfo"
printf '7f00 r-xp 0 08:01 1 /usr/lib/x86_64-linux-gnu/libjemalloc.so.2\n7f01 r-xp 0 08:01 2 /usr/lib/x86_64-linux-gnu/libc.so.6\n' >"$PROC/101/maps"

fake_process 202 rnode.bin 8192 22 9
printf '/tmp/rnode.bin\0run\0--config-file\0/c/rnode.conf\0--data-dir\0/w/.subprocess-data/s1/validator1\0' >"$PROC/202/cmdline"
printf '0::/system.slice/cloud-final.service\n' >"$PROC/202/cgroup"
printf '29 1 8:1 / / rw,relatime - ext4 /dev/sda1 rw\n40 29 8:1 /w /w rw,relatime - ext4 /dev/sda1 rw\n' >"$PROC/202/mountinfo"
printf '7f00 r-xp 0 08:01 3 /tmp/libjemalloc.so.2\n7f01 r-xp 0 08:01 4 /usr/lib/x86_64-linux-gnu/libssl.so.3\n' >"$PROC/202/maps"

fake_process 303 python3 1 1 1
printf 'python3\0-m\0pytest\0run\0' >"$PROC/303/cmdline"
printf '0::/\n' >"$PROC/303/cgroup"

# Node 120 stands for a benchmark shard that runs across iterations: it has
# the role validator1 too, and it is running before the sampler starts.
fake_process 120 rnode.bin 2048 6 0
printf '/tmp/rnode.bin\0run\0--data-dir\0/w/.subprocess-data/bench/validator1\0' >"$PROC/120/cmdline"
printf '0::/system.slice/cloud-final.service\n' >"$PROC/120/cgroup"

fake_process 150 rnode.bin 1024 5 0
printf '/tmp/rnode.bin\0run\0--data-dir\0/w/.subprocess-data/s1/validator10\0' >"$PROC/150/cmdline"
printf '0::/system.slice/cloud-final.service\n' >"$PROC/150/cgroup"

mkdir -p "$PROC/sys/vm"
printf '20\n' >"$PROC/sys/vm/dirty_ratio"
printf '10\n' >"$PROC/sys/vm/dirty_background_ratio"

mkdir -p "$TMP/bin"
cat >"$TMP/bin/docker" <<SH
#!/usr/bin/env bash
printf '%s\n' "\$*" >>"\$DOCKER_LOG"
[ "\$1 \$4" = "inspect $CID" ] && printf '/rnode.test.abc.validator2\n'
SH
cat >"$TMP/bin/strace" <<'SH'
#!/usr/bin/env bash
printf 'strace %s\n' "$*" >>"$STRACE_LOG"
if [ "${STRACE_NEEDS_SUDO:-0}" = 1 ] && [ "${FAKE_SUDO:-0}" != 1 ]; then
	printf 'attach: ptrace(PTRACE_SEIZE, 202): Operation not permitted\n' >&2
	exit 1
fi
sleep "${STRACE_SLEEP:-0}"
printf '%% time     seconds  usecs/call     calls    errors syscall\n100.00    0.010000          10      1000           fdatasync\n'
SH
cat >"$TMP/bin/fake-sudo" <<'SH'
#!/usr/bin/env bash
[ "$1" = -n ] && shift
FAKE_SUDO=1 exec "$@"
SH
chmod +x "$TMP/bin/docker" "$TMP/bin/strace" "$TMP/bin/fake-sudo"

run_sampler() {  # run_sampler ITERATION_DIR SUDO EXTRA_ENV...
	local iter="$1" sudo="$2"
	shift 2
	env PATH="$TMP/bin:$PATH" PROC_ROOT="$PROC" SOAK_IO_SUDO="$sudo" SOAK_IO_DOCKER="$TMP/bin/docker" \
		SOAK_IO_STRACE="$TMP/bin/strace" STRACE_LOG="$TMP/strace.log" DOCKER_LOG="$TMP/docker.log" \
		SOAK_IO_INTERVAL_SECONDS=1 SOAK_IO_STRACE_DELAY_SECONDS=0 SOAK_IO_STRACE_SECONDS=10 "$@" bash "$TOOL" "$iter" &
	SAMPLER=$!
}

# The iteration starts its shard after the sampler: node 202 appears only after
# the first sample.
hide_iteration_node() { mv "$PROC/202" "$TMP/node-202"; }
start_iteration_node() {  # start_iteration_node IO_TSV
	for _ in $(seq 1 50); do
		[ -f "$1" ] && [ "$(wc -l <"$1")" -ge 2 ] && break
		sleep 0.1
	done
	mv "$TMP/node-202" "$PROC/202"
}

ITER="$TMP/iteration-00001-subprocess"
mkdir -p "$ITER"
printf -- '--- Phase: low ---\n--- Phase: sustained ---\n' >"$ITER/pytest.log"
: >"$TMP/strace.log"
: >"$TMP/docker.log"
hide_iteration_node
run_sampler "$ITER" "" STRACE_SLEEP=6
OUT="$ITER/io-diagnostics"
start_iteration_node "$OUT/io.tsv"
for _ in $(seq 1 50); do
	[ -s "$TMP/strace.log" ] && [ "$(awk -F'\t' '$2 == 202' "$OUT/io.tsv" | wc -l)" -ge 4 ] && break
	sleep 0.2
done
[ "$(awk -F'\t' '$2 == 202' "$OUT/io.tsv" | wc -l)" -ge 4 ] ||
	fail "sampling stopped while strace ran: $(cat "$OUT/io.tsv")"
pgrep -f "$TMP/bin/strace" >/dev/null || fail "strace ended before the sampling check, so the check cannot show that sampling continued"
stop_started="$(date +%s)"
kill "$SAMPLER" 2>/dev/null || true
wait "$SAMPLER" 2>/dev/null || true
[ "$(($(date +%s) - stop_started))" -le 2 ] || fail "the sampler did not stop promptly during strace"
sleep 0.5
if pgrep -f "$TMP/bin/strace" >/dev/null; then fail "strace kept running after the sampler stopped"; fi

has_row() {
	local want
	want="$(printf "$1")"
	awk -v want="$want" '{ sub(/^[0-9]+\t/, ""); if ($0 == want) found = 1 } END { exit !found }' "$OUT/io.tsv"
}
has_row "202	validator1	10	20	3	4	0	8192	7	22	9" ||
	fail "the subprocess node sample is wrong: $(cat "$OUT/io.tsv")"
has_row "101	rnode.test.abc.validator2	10	20	3	4	0	4096	7	11	1" ||
	fail "the Docker node sample is wrong: $(cat "$OUT/io.tsv")"
if awk -F'\t' '$2 == 303 { found = 1 } END { exit !found }' "$OUT/io.tsv"; then fail "a non-node process was sampled"; fi
grep -Fq 'mount: 40 29 8:1 /w /w rw,relatime - ext4 /dev/sda1 rw' "$OUT/nodes.txt" ||
	fail "the subprocess data directory mount is wrong: $(cat "$OUT/nodes.txt")"
grep -Fq 'mount: 36 25 0:31 /volumes/v1/_data /var/lib/rnode rw,relatime - ext4 /dev/sda1 rw' "$OUT/nodes.txt" ||
	fail "the Docker data directory mount is wrong: $(cat "$OUT/nodes.txt")"
grep -Fq '/tmp/libjemalloc.so.2' "$OUT/nodes.txt" || fail "the subprocess libraries are missing"
[ "$(grep -c '^== ' "$OUT/nodes.txt")" -eq 4 ] || fail "each node must be described once: $(grep '^== ' "$OUT/nodes.txt")"
grep -Fxq 'dirty_ratio = 20' "$OUT/host.txt" || fail "the host dirty settings are missing: $(cat "$OUT/host.txt")"
[ "$(wc -l <"$TMP/docker.log")" -eq 1 ] || fail "docker inspect must run once for each container: $(cat "$TMP/docker.log")"
[ "$(wc -l <"$TMP/strace.log")" -eq 1 ] || fail "strace must run once: $(cat "$TMP/strace.log")"
grep -Fq -- '-p 202 ' "$TMP/strace.log" || fail "strace did not target validator1 of the iteration: $(cat "$TMP/strace.log")"
grep -A1 -Fx '== validator1 (pid 120)' "$OUT/nodes.txt" | grep -Fxq 'running at sampler start: yes' ||
	fail "the benchmark node is not marked as running at sampler start: $(cat "$OUT/nodes.txt")"
grep -A1 -Fx '== validator1 (pid 202)' "$OUT/nodes.txt" | grep -Fxq 'running at sampler start: no' ||
	fail "the iteration node is marked as running at sampler start: $(cat "$OUT/nodes.txt")"
grep -Fxq 'trigger: sustained phase marker plus 0s' "$OUT/strace-validator1.txt" ||
	fail "the strace trigger is not recorded: $(cat "$OUT/strace-validator1.txt")"

ITER2="$TMP/iteration-00002-subprocess"
mkdir -p "$ITER2"
printf -- '--- Phase: sustained ---\n' >"$ITER2/pytest.log"
: >"$TMP/strace.log"
hide_iteration_node
run_sampler "$ITER2" "$TMP/bin/fake-sudo -n" STRACE_NEEDS_SUDO=1
start_iteration_node "$ITER2/io-diagnostics/io.tsv"
for _ in $(seq 1 50); do
	grep -q fdatasync "$ITER2/io-diagnostics/strace-validator1.txt" 2>/dev/null && break
	sleep 0.2
done
kill "$SAMPLER" 2>/dev/null || true
wait "$SAMPLER" 2>/dev/null || true
grep -Fq 'fdatasync' "$ITER2/io-diagnostics/strace-validator1.txt" || fail "the sudo strace summary is missing"
if grep -Fq 'Operation not permitted' "$ITER2/io-diagnostics/strace-validator1.txt"; then
	fail "the failed direct attempt polluted the strace output"
fi

printf 'PASS: sample-node-io records the I/O, faults, mounts, libraries, and one exact-role strace summary of a node that started after the sampler, and keeps sampling during strace.\n'
