#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SOURCE_DIR="$ROOT/formal/mcrl2/uptime"
WORK_DIR="$ROOT/target/verification/uptime/mcrl2"
mkdir -p "$WORK_DIR"

for tool in mcrl22lps lps2lts lps2pbes pbes2bool; do
  command -v "$tool" >/dev/null 2>&1 || {
    echo "error: required uptime process-algebra tool is unavailable: $tool" >&2
    exit 1
  }
done

for source in concurrent_service.mcrl2 global_mutex_unsafe.mcrl2 no_deadlock.mcf parallel_validation.mcf parallel_replay.mcf; do
  test -s "$SOURCE_DIR/$source" || {
    echo "error: missing or empty uptime process-algebra source: $source" >&2
    exit 1
  }
done

mcrl22lps "$SOURCE_DIR/concurrent_service.mcrl2" "$WORK_DIR/concurrent_service.lps" >"$WORK_DIR/concurrent_service-translate.log" 2>&1
lps2lts "$WORK_DIR/concurrent_service.lps" "$WORK_DIR/concurrent_service.lts" >"$WORK_DIR/concurrent_service-lts.log" 2>&1

check_true() {
  local formula="$1" name="$2" result
  lps2pbes -f "$formula" "$WORK_DIR/concurrent_service.lps" "$WORK_DIR/$name.pbes" >"$WORK_DIR/$name-pbes.log" 2>&1
  result="$(pbes2bool "$WORK_DIR/$name.pbes" 2>&1 | tail -n 1)"
  test "$result" = "true" || {
    echo "error: uptime process property $name returned $result, expected true" >&2
    exit 1
  }
}

check_true "$SOURCE_DIR/no_deadlock.mcf" no_deadlock
check_true "$SOURCE_DIR/parallel_validation.mcf" parallel_validation
check_true "$SOURCE_DIR/parallel_replay.mcf" parallel_replay

mcrl22lps "$SOURCE_DIR/global_mutex_unsafe.mcrl2" "$WORK_DIR/global_mutex_unsafe.lps" >"$WORK_DIR/global-mutex-translate.log" 2>&1
lps2pbes -f "$SOURCE_DIR/parallel_validation.mcf" "$WORK_DIR/global_mutex_unsafe.lps" "$WORK_DIR/global_mutex_unsafe.pbes" >"$WORK_DIR/global-mutex-pbes.log" 2>&1
unsafe_result="$(pbes2bool "$WORK_DIR/global_mutex_unsafe.pbes" 2>&1 | tail -n 1)"
test "$unsafe_result" = "false" || {
  echo "error: global-mutex negative control returned $unsafe_result, expected false" >&2
  exit 1
}

echo "mCRL2 uptime concurrency verification passed."
