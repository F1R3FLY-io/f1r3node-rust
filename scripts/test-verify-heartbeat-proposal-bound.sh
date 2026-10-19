#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
export H1_VERIFY_CALLS="$tmp/calls"
cat > "$tmp/cargo" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$*" >> "$H1_VERIFY_CALLS"
SH
chmod +x "$tmp/cargo"
export PATH="$tmp:$PATH"

bash "$script_dir/verify-heartbeat-proposal-bound.sh" node/src/rust/instances/heartbeat_proposer.rs
bash "$script_dir/verify-heartbeat-proposal-bound.sh" casper/src/rust/blocks/proposer/proposer.rs
expected="$(printf '%s\n' \
    'test --release -p node heartbeat_proposer' \
    'test --release -p casper --test mod add_block::h1_proposer_spec')"
test "$(cat "$H1_VERIFY_CALLS")" = "$expected"

set +e
bash "$script_dir/verify-heartbeat-proposal-bound.sh" unsupported > "$tmp/unsupported" 2>&1
status=$?
set -e
test "$status" -eq 2
test "$(wc -l < "$H1_VERIFY_CALLS" | tr -d ' ')" -eq 2
printf '%s\n' 'Heartbeat verification dispatch tests passed.'
