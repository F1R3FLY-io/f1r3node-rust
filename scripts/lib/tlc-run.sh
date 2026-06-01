#!/usr/bin/env bash
# scripts/lib/tlc-run.sh
#
# Sourced helper: run TLC (the TLA+ model checker) under a strict memory
# envelope so that a large state space can never exhaust system RAM.
#
# Post-mortem (2026-05-31): a SlashFlow run wrote a ~13 GB on-disk state
# graph into /tmp — which is tmpfs (RAM-backed, half of this 128 GB host)
# — under the JVM's unbounded ergonomic heap (MaxRAMPercentage=25% ≈ 30 GB
# here) with `-workers auto` (= 64 worker threads), and locked the machine.
# None of the runner scripts actually applied the resource cap their
# banners claimed. This helper makes the cap real, for EVERY TLC run:
#
#   1. METADIR ON REAL DISK — TLC's state-queue / fingerprint graph lands
#      under $TLC_METADIR_ROOT (default <repo>/target/tlc-metadir, on the
#      NVMe), NEVER TMPDIR (/tmp is tmpfs = RAM here). This is the file set
#      that grew to 13 GB; on disk it costs disk, in tmpfs it costs RAM.
#   2. BOUNDED JVM HEAP — -Xmx$TLC_HEAP (default 4g), instead of the JVM's
#      ergonomic ~30 GB. Bounded models keep < 1 GB of fingerprints.
#   3. BOUNDED WORKERS — -workers $TLC_WORKERS (default 4), NEVER `auto`
#      (= 64 threads here), which multiplies the peak in-flight frontier.
#   4. HARD cgroup CEILING — systemd-run --user --scope -p MemoryMax=$TLC_RSS
#      -p MemorySwapMax=0 (so a runaway is killed cleanly at the cap rather
#      than OOM-ing the host, and cannot thrash the 512 GB swap). Falls back
#      to `prlimit --as`; if neither is available it refuses to run unless
#      ALLOW_UNBOUNDED_TLC=1.
#
# Overridable knobs (environment):
#   TLC_HEAP=4g          JVM -Xmx (use a JVM size suffix: g/m)
#   TLC_WORKERS=4        TLC worker threads (a number; never `auto`)
#   TLC_RSS=12G          cgroup MemoryMax (systemd size suffix: G/M)
#   TLC_METADIR_ROOT=<repo>/target/tlc-metadir
#   TLC_JAR=/usr/share/java/tla2tools.jar
#   ALLOW_UNBOUNDED_TLC=1   debug escape hatch — skips the cgroup ceiling
#
# API:
#   tlc_bounded <cmd...>                        run cmd under the cgroup ceiling
#   tlc_metadir <name>                          echo an on-disk metadir (mkdir -p'd)
#   tlc_run <metadir> <config> <module> [args]  run TLC: bounded heap+workers+disk
#
# This file is SOURCED, not executed; it defines functions and defaults and
# must not enable `set -e` of its own (the sourcing script owns shell flags).

TLC_HEAP="${TLC_HEAP:-4g}"
TLC_WORKERS="${TLC_WORKERS:-4}"
TLC_RSS="${TLC_RSS:-12G}"
TLC_JAR="${TLC_JAR:-/usr/share/java/tla2tools.jar}"

# Resolve the repo root. Callers SHOULD export TLC_REPO_ROOT (each computes
# it correctly); otherwise fall back to `git rev-parse` from this file's
# directory (symlink-proof), then to a path-relative climb. The metadir
# then defaults onto the in-repo NVMe target/, never TMPDIR (tmpfs = RAM).
__tlc_lib_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
if [[ -z "${TLC_REPO_ROOT:-}" ]]; then
  TLC_REPO_ROOT="$(git -C "$__tlc_lib_dir" rev-parse --show-toplevel 2>/dev/null || true)"
  [[ -n "$TLC_REPO_ROOT" ]] || TLC_REPO_ROOT="$(cd "$__tlc_lib_dir/../.." && pwd)"
fi
TLC_METADIR_ROOT="${TLC_METADIR_ROOT:-$TLC_REPO_ROOT/target/tlc-metadir}"

# Convert a systemd-style size (e.g. 12G, 512M) to bytes for the prlimit
# fallback path.
__tlc_mem_bytes() {
  local value="$1" number suffix
  number="${value%[KkMmGg]}"
  suffix="${value:${#number}}"
  case "$suffix" in
    [Kk]) echo $((number * 1024)) ;;
    [Mm]) echo $((number * 1024 * 1024)) ;;
    [Gg]) echo $((number * 1024 * 1024 * 1024)) ;;
    "")   echo "$number" ;;
    *) echo "tlc-run: unsupported memory suffix in '$value'" >&2; return 1 ;;
  esac
}

# tlc_bounded <cmd...> — exec cmd under a hard memory ceiling ($TLC_RSS).
# Trace lines go to stderr so a caller capturing stdout sees only TLC output.
tlc_bounded() {
  if [[ "${ALLOW_UNBOUNDED_TLC:-0}" == "1" ]]; then
    echo "+ (unbounded) $*" >&2
    "$@"
    return
  fi
  if command -v systemd-run >/dev/null 2>&1 && systemd-run --user --scope true >/dev/null 2>&1; then
    echo "+ systemd-run --user --scope -p MemoryMax=$TLC_RSS -p MemorySwapMax=0 -- $*" >&2
    systemd-run --user --scope -p "MemoryMax=$TLC_RSS" -p "MemorySwapMax=0" -- "$@"
    return
  fi
  if command -v prlimit >/dev/null 2>&1; then
    local bytes
    bytes="$(__tlc_mem_bytes "$TLC_RSS")" || return 1
    echo "+ prlimit --as=$bytes -- $*" >&2
    prlimit --as="$bytes" -- "$@"
    return
  fi
  echo "tlc-run: cannot bound TLC to $TLC_RSS (no systemd-run/prlimit); set ALLOW_UNBOUNDED_TLC=1 to override" >&2
  return 1
}

# tlc_metadir <name> — an on-disk (NVMe) metadir path, created on demand.
tlc_metadir() {
  local dir="$TLC_METADIR_ROOT/$1"
  mkdir -p "$dir"
  printf '%s' "$dir"
}

# tlc_run <metadir> <config> <module> [extra tlc args...]
# Run TLC with a bounded heap, bounded workers, and the given ON-DISK
# metadir, all under the cgroup ceiling. Forwards TLC's stdout/stderr and
# propagates its exit code, so callers may `out=$(tlc_run ... 2>&1)` or
# `tlc_run ... >log 2>&1`.
tlc_run() {
  local metadir="$1" config="$2" module="$3"
  shift 3
  mkdir -p "$metadir"
  if [[ -f "$TLC_JAR" ]]; then
    # Preferred: java with an explicit -Xmx (no env-var gymnastics).
    tlc_bounded java "-Xmx$TLC_HEAP" -XX:+UseParallelGC -cp "$TLC_JAR" tlc2.TLC \
      -workers "$TLC_WORKERS" -metadir "$metadir" -config "$config" "$@" "$module"
  elif command -v tlc >/dev/null 2>&1; then
    # Fallback: the `tlc` wrapper honours TLC_JAVA_OPTS for the JVM heap.
    # Export inside a subshell so the assignment reaches the wrapped java.
    (
      export TLC_JAVA_OPTS="-Xmx$TLC_HEAP ${TLC_JAVA_OPTS:-}"
      tlc_bounded tlc -workers "$TLC_WORKERS" -metadir "$metadir" -config "$config" "$@" "$module"
    )
  else
    echo "tlc-run: no TLC jar at $TLC_JAR and no 'tlc' on PATH" >&2
    return 3
  fi
}
