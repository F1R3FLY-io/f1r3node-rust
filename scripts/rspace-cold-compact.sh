#!/usr/bin/env bash
# Offline cold-store compaction (issue #1181, Phase 5).
#
# Phase-4's live sweep deletes orphan cold entries in place; LMDB
# reuses the freed pages for future writes but does not return them
# to the filesystem. After a long run with a shifting retention
# window, the on-disk footprint can be much larger than the live
# data. `mdb_copy -c` rewrites the env into a dense copy; this
# script wraps it with the safety checks and atomic swap operators
# expect.
#
# USAGE:
#   scripts/rspace-cold-compact.sh --data-dir /var/lib/rnode [--dry-run]
#
# PRECONDITIONS:
#   * The node process MUST be stopped. The script refuses if
#     `rspace/cold/lock.mdb` is held by any process.
#   * `mdb_copy` AND `mdb_stat` must be on PATH (typically
#     `lmdb-utils` package on Linux, `lmdb` Homebrew formula on
#     macOS). `mdb_stat` is used to verify the compacted output
#     before the swap.
#   * `lsof` must be on PATH, for the node-running check. Fail-open
#     here would risk corruption if an operator ran this on a live
#     node from a container image without lsof installed.
#   * The data-dir filesystem must have free space ≈ the current
#     cold data.mdb size — the compacted copy is written alongside
#     before the swap.
#
# The compacted copy is written to `rspace/cold.new/` and swapped in
# via POSIX directory rename (atomic within one filesystem). On any
# failure before the final swap, the original `rspace/cold/` is
# untouched. After a successful swap the prior `rspace/cold.old/`
# is deleted; keep it (by passing `--keep-old`) if you want to
# compare sizes manually before removing.

set -euo pipefail

DATA_DIR=""
DRY_RUN=false
KEEP_OLD=false

usage() {
    cat <<EOF
Usage: $0 --data-dir <path> [--dry-run] [--keep-old]

Compact the rspace/cold LMDB env at <path>/rspace/cold using
\`mdb_copy -c\` and atomically swap. The node must be stopped.

Options:
  --data-dir <path>   Path to the node's data directory (contains rspace/).
  --dry-run           Report size + plan without modifying anything.
  --keep-old          Leave rspace/cold.old in place after a successful swap.

Exit codes:
  0  Compaction succeeded (or skipped because copy was not smaller).
  1  Precondition failure (missing dir, node running, mdb_copy absent).
  2  Compaction failed partway; original rspace/cold is untouched.
EOF
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --data-dir)
            [[ $# -ge 2 ]] || { echo "error: --data-dir requires an argument" >&2; exit 1; }
            DATA_DIR="$2"
            shift 2
            ;;
        --dry-run)
            DRY_RUN=true
            shift
            ;;
        --keep-old)
            KEEP_OLD=true
            shift
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            echo "error: unknown argument $1" >&2
            usage >&2
            exit 1
            ;;
    esac
done

if [[ -z "$DATA_DIR" ]]; then
    echo "error: --data-dir is required" >&2
    usage >&2
    exit 1
fi

COLD_DIR="$DATA_DIR/rspace/cold"
NEW_DIR="$DATA_DIR/rspace/cold.new"
OLD_DIR="$DATA_DIR/rspace/cold.old"
DATA_FILE="$COLD_DIR/data.mdb"
LOCK_FILE="$COLD_DIR/lock.mdb"

# ---- Preconditions ----

for tool in mdb_copy mdb_stat lsof; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        echo "error: $tool not found on PATH. Install lmdb-utils (Linux) or lmdb + \
lsof (brew)." >&2
        exit 1
    fi
done

[[ -d "$COLD_DIR" ]] || { echo "error: $COLD_DIR does not exist" >&2; exit 1; }
[[ -f "$DATA_FILE" ]] || { echo "error: $DATA_FILE not found" >&2; exit 1; }

if [[ -e "$NEW_DIR" ]]; then
    echo "error: $NEW_DIR already exists (prior run left residue). Remove it and retry." >&2
    exit 1
fi
if [[ -e "$OLD_DIR" ]]; then
    echo "error: $OLD_DIR already exists (prior run left residue). Remove it and retry." >&2
    exit 1
fi

# Node-running check. LMDB holds a POSIX fcntl lock on lock.mdb via
# mdb_env_open; while fcntl locks aren't exposed by `lsof` directly,
# LMDB also keeps the fd open while the env is open, and `lsof`
# reliably reports that. A node process that has crashed while
# holding the lock does not — that's the correct behavior anyway,
# since those crashed-state locks are stale.
if [[ -f "$LOCK_FILE" ]] && lsof -- "$LOCK_FILE" >/dev/null 2>&1; then
    echo "error: $LOCK_FILE is held by another process. Stop the node first." >&2
    exit 1
fi

# Portable mode-preservation: GNU `stat -c`, BSD `stat -f`, or
# nothing (bail). Only used for octal permission bits, so both
# forms return a three- or four-digit string like "750".
preserve_mode_of() {
    local src="$1" dst="$2"
    local mode
    if mode=$(stat -c '%a' "$src" 2>/dev/null); then
        chmod "$mode" "$dst"
    elif mode=$(stat -f '%Lp' "$src" 2>/dev/null); then
        chmod "$mode" "$dst"
    else
        echo "warn: could not read mode of $src; $dst may have different permissions" >&2
    fi
}

# ---- Size reporting ----

# Portable byte count for data.mdb: try BSD stat, fall back to GNU.
if before_bytes=$(stat -f%z "$DATA_FILE" 2>/dev/null); then
    :
else
    before_bytes=$(stat -c%s "$DATA_FILE")
fi

echo "cold-store compaction (issue #1181):"
echo "  data dir:    $DATA_DIR"
echo "  current:     $DATA_FILE ($before_bytes bytes)"

if $DRY_RUN; then
    echo "  dry-run:     would run \`mdb_copy -c $COLD_DIR $NEW_DIR\` and swap."
    exit 0
fi

# ---- Compaction ----

mkdir "$NEW_DIR"
preserve_mode_of "$COLD_DIR" "$NEW_DIR"
# Catch shell errors (set -e), operator Ctrl-C, and SIGTERM. Without
# the signal catches, a mid-run abort would leave $NEW_DIR behind
# and poison the next invocation's residue check.
cleanup_new_dir() { rm -rf -- "$NEW_DIR"; }
trap cleanup_new_dir ERR INT TERM

echo "  running:     mdb_copy -c $COLD_DIR $NEW_DIR"
mdb_copy -c "$COLD_DIR" "$NEW_DIR"

if after_bytes=$(stat -f%z "$NEW_DIR/data.mdb" 2>/dev/null); then
    :
else
    after_bytes=$(stat -c%s "$NEW_DIR/data.mdb")
fi
if [[ $after_bytes -lt $before_bytes ]]; then
    freed=$((before_bytes - after_bytes))
    echo "  compacted:   $NEW_DIR/data.mdb ($after_bytes bytes; $freed bytes freed)"
else
    echo "  compacted:   $NEW_DIR/data.mdb ($after_bytes bytes; not smaller than source)"
fi

# Integrity check. `mdb_stat` opens the compacted env and reads its
# header + root metadata. Catches tool-level corruption (mdb_copy
# bugs, disk faults during the copy) before we swap.
echo "  verifying:   mdb_stat $NEW_DIR"
if ! mdb_stat "$NEW_DIR" >/dev/null 2>&1; then
    echo "error: compacted env failed mdb_stat integrity check; refusing to swap" >&2
    rm -rf -- "$NEW_DIR"
    trap - ERR INT TERM
    exit 2
fi
preserve_mode_of "$DATA_FILE" "$NEW_DIR/data.mdb"

if [[ $after_bytes -ge $before_bytes ]]; then
    echo "  result:      compacted copy is not smaller; nothing to reclaim. Removing \
$NEW_DIR without swapping."
    rm -rf -- "$NEW_DIR"
    trap - ERR INT TERM
    exit 0
fi

# ---- Atomic swap ----

# Disarm traps: from this point on, failures leave recoverable
# state we want to preserve for operator inspection, not blindly
# `rm -rf $NEW_DIR`.
trap - ERR INT TERM

mv -- "$COLD_DIR" "$OLD_DIR"
# If the next mv fails, the operator is left with no $COLD_DIR but
# $OLD_DIR intact. They can mv it back.
if ! mv -- "$NEW_DIR" "$COLD_DIR"; then
    echo "error: second rename failed; restoring from $OLD_DIR" >&2
    mv -- "$OLD_DIR" "$COLD_DIR"
    rm -rf -- "$NEW_DIR"
    exit 2
fi

if $KEEP_OLD; then
    echo "  swap:        done. Previous data at $OLD_DIR (kept per --keep-old)."
else
    rm -rf -- "$OLD_DIR"
    echo "  swap:        done. Previous data removed."
fi

echo "cold-store compaction complete. Start the node to resume."
