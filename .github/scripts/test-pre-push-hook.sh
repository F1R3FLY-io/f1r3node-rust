#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
HOOK="$ROOT/.githooks/pre-push"
HARNESS="$(mktemp -d)"
trap 'rm -rf "$HARNESS"' EXIT
mkdir -p "$HARNESS/bin" "$HARNESS/tmp"
REAL_GIT="$(command -v git)"

cat >"$HARNESS/bin/git" <<EOF
#!/usr/bin/env bash
set -euo pipefail
case "\${1:-} \${2:-}" in
  "rev-parse --show-toplevel") exec "$REAL_GIT" "\$@" ;;
  "ls-remote origin") printf '%s\t%s\n' "\$FAKE_REMOTE_OID" "\${3:-refs/heads/test}" ;;
  "fetch origin") exit 0 ;;
  *) exec "$REAL_GIT" "\$@" ;;
esac
EOF
cat >"$HARNESS/bin/timeout" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$*" >> "$FAKE_TIMEOUT_LOG"
shift
exec "$@"
EOF
cat >"$HARNESS/bin/cargo" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
printf '%s %s\n' "$$" "$*" >> "$FAKE_CARGO_LOG"
if [[ "${SLEEP_CASPER:-0}" == 1 && "$*" == "test --release -p casper" ]]; then
    : > "$FAKE_CASPER_STARTED"
    sleep 300
fi
EOF
chmod +x "$HARNESS/bin/git" "$HARNESS/bin/timeout" "$HARNESS/bin/cargo"
grep -Fq "trap 'cleanup 130' INT" "$HOOK"

HEAD_SHA="$(git -C "$ROOT" rev-parse HEAD)"
REMOTE_SHA="0000000000000000000000000000000000000001"
PUSH_INPUT="refs/heads/test $HEAD_SHA refs/heads/test $REMOTE_SHA"
COMMON_ENV=(
  env
  PATH="$HARNESS/bin:$PATH"
  TMPDIR="$HARNESS/tmp"
  FAKE_REMOTE_OID="$REMOTE_SHA"
  FAKE_TIMEOUT_LOG="$HARNESS/timeouts"
  FAKE_CARGO_LOG="$HARNESS/cargo"
  CI=
  GITLAB_CI=
  GITHUB_ACTIONS=
  SKIP_CLIPPY=1
  SKIP_DENY=1
  SKIP_CI_TESTS=1
  TEST_CRATES=casper
)

printf '%s\n' "$PUSH_INPUT" |
  "${COMMON_ENV[@]}" bash "$HOOK" origin fake >"$HARNESS/normal.out" 2>&1
grep -q '^1800 cargo test --release -p casper$' "$HARNESS/timeouts"
if find "$HARNESS/tmp" -mindepth 1 -print -quit | grep -q .; then
  echo "normal hook exit left temporary results" >&2
  exit 1
fi

: >"$HARNESS/timeouts"
: >"$HARNESS/cargo"
printf '%s\n' "$PUSH_INPUT" |
  "${COMMON_ENV[@]}" SLEEP_CASPER=1 FAKE_CASPER_STARTED="$HARNESS/started" \
    bash "$HOOK" origin fake >"$HARNESS/cancel.out" 2>&1 &
HOOK_PID=$!
for _ in $(seq 1 100); do
  [[ -f "$HARNESS/started" ]] && break
  sleep 0.05
done
[[ -f "$HARNESS/started" ]]
kill -TERM "$HOOK_PID"
set +e
wait "$HOOK_PID"
RC=$?
set -e
[[ "$RC" -eq 143 ]]
if grep -q 'No such file or directory' "$HARNESS/cancel.out"; then
  echo "hook cancellation removed results before workers stopped" >&2
  exit 1
fi
sleep 0.2
while read -r pid _; do
  if kill -0 "$pid" 2>/dev/null; then
    echo "hook cancellation left cargo process $pid" >&2
    exit 1
  fi
done <"$HARNESS/cargo"
if find "$HARNESS/tmp" -mindepth 1 -print -quit | grep -q .; then
  echo "hook cancellation left temporary results" >&2
  exit 1
fi

printf 'pre-push hook tests passed\n'
