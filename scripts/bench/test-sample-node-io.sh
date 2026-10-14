#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
TOOL="$ROOT/scripts/bench/sample-node-io.sh"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

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

mkdir -p "$PROC/sys/vm"
printf '20\n' >"$PROC/sys/vm/dirty_ratio"
printf '10\n' >"$PROC/sys/vm/dirty_background_ratio"

mkdir -p "$TMP/bin"
cat >"$TMP/bin/docker" <<SH
#!/usr/bin/env bash
[ "\$1 \$4" = "inspect $CID" ] && printf '/rnode.test.abc.validator2\n'
SH
cat >"$TMP/bin/strace" <<'SH'
#!/usr/bin/env bash
printf 'strace %s\n' "$*" >>"$STRACE_LOG"
printf '%% time     seconds  usecs/call     calls    errors syscall\n100.00    0.010000          10      1000           fdatasync\n'
SH
chmod +x "$TMP/bin/docker" "$TMP/bin/strace"

ITER="$TMP/iteration-00001-subprocess"
mkdir -p "$ITER"
printf -- '--- Phase: low ---\n--- Phase: sustained ---\n' >"$ITER/pytest.log"

PATH="$TMP/bin:$PATH" PROC_ROOT="$PROC" SOAK_IO_SUDO="" SOAK_IO_DOCKER="$TMP/bin/docker" \
	SOAK_IO_STRACE="$TMP/bin/strace" STRACE_LOG="$TMP/strace.log" SOAK_IO_INTERVAL_SECONDS=1 \
	SOAK_IO_STRACE_DELAY_SECONDS=0 SOAK_IO_STRACE_SECONDS=2 bash "$TOOL" "$ITER" &
sampler=$!
for _ in $(seq 1 50); do
	[ -s "$ITER/io-diagnostics/strace-validator1.txt" ] && [ "$(wc -l <"$ITER/io-diagnostics/io.tsv")" -ge 5 ] && break
	sleep 0.2
done
kill "$sampler" 2>/dev/null || true
wait "$sampler" 2>/dev/null || true

OUT="$ITER/io-diagnostics"
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
[ "$(grep -c '^== ' "$OUT/nodes.txt")" -eq 2 ] || fail "each node must be described once: $(grep '^== ' "$OUT/nodes.txt")"
grep -Fxq 'dirty_ratio = 20' "$OUT/host.txt" || fail "the host dirty settings are missing: $(cat "$OUT/host.txt")"
grep -Fq 'fdatasync' "$OUT/strace-validator1.txt" || fail "the strace summary is missing"
[ "$(wc -l <"$TMP/strace.log")" -eq 1 ] || fail "strace must run once: $(cat "$TMP/strace.log")"
grep -Fq -- '-p 202 ' "$TMP/strace.log" || fail "strace did not target validator1: $(cat "$TMP/strace.log")"

printf 'PASS: sample-node-io records the I/O, faults, mounts, libraries, and one strace summary of the node processes.\n'
