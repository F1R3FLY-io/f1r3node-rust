#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
REPO="$WORK/repo"
mkdir -p "$REPO/scripts/ci" "$REPO/docs/claims"
cp "$ROOT/scripts/ci/test-soak-claim-inventory.sh" "$REPO/scripts/ci/"

ruby -rjson -rdigest -rfileutils - "$REPO" "$WORK" <<'RUBY'
root, work = ARGV
input = 'input/*keep*/source.txt'
FileUtils.mkdir_p(File.dirname(File.join(root, input)))
File.write(File.join(root, input), 'fixture')
check_id = 'FIXTURE'
evidence_id = 'FIXTURE-EVIDENCE'
check = {
  id: check_id, command: 'fixture', configurations: [input],
  verifier: { name: 'fixture', version: 'fixture' },
  assumptions: ['fixture'], bounds: ['fixture'],
  production_tests: [{ path: input, command: 'fixture', result: 'not-run' }],
  ci: { workflow: input, job: 'fixture', enforcement: 'pending' },
  evidence_refs: [evidence_id], result: 'not-run', pending_obligations: ['pending']
}
claims = %w[CLAIM-FINALITY-001 CLAIM-FINALITY-002 CLAIM-SOAK-GATE-001 CLAIM-SOAK-001].map do |id|
  { id: id, declared_status: 'pending', candidate_status: 'pending', specification: input,
    implementation: [{ path: input, symbols: ['fixture'] }], checks: [check_id], pending_obligations: ['pending'] }
end
data = {
  schema_version: 1,
  candidate: { base_commit: '0' * 40, binding: 'source-sha256', inputs_sha256: { input => Digest::SHA256.file(File.join(root, input)).hexdigest } },
  claims: claims, checks: [check],
  evidence: { evidence_id => { path: input, role: 'historical-baseline', identity: 'https://example.test/a//b', limitations: ['fixture'] } },
  gates: %w[G0 D1 D2 O1 F1 F2 F3 D3 A1].map { |id| { id: id, status: 'pending', checks: [check_id], pending_obligations: ['pending'] } },
  acceptance: {
    status: 'pending', pending_obligations: ['pending'],
    identity: %w[workflow_control_sha node_sha image_digest harness_sha configuration workload_profile run_id run_attempt].to_h { |key| [key, nil] }
  }
}
plain = JSON.pretty_generate(data)
commented = "// Inventory fixture\n/* Block comment */\n" + plain.sub('"schema_version": 1,', '"schema_version": /* inline comment */ 1, // line comment') + "\n// End comment\n"
File.write(File.join(root, 'docs/claims/soak-claim-inventory.jsonc'), commented)
File.write(File.join(work, 'explicit.jsonc'), commented)
File.write(File.join(work, 'legacy.json'), plain)
File.write(File.join(work, 'unterminated.jsonc'), plain + ' /* missing end')
File.write(File.join(work, 'invalid.jsonc'), '{"schema_version": /* missing value */ }')
File.write(File.join(work, 'trailing-garbage.jsonc'), commented + 'invalid')
stale = Marshal.load(Marshal.dump(data))
stale[:candidate][:inputs_sha256][input] = '0' * 64
File.write(File.join(work, 'stale.jsonc'), "// Stale binding\n" + JSON.pretty_generate(stale))
incomplete = Marshal.load(Marshal.dump(data))
incomplete[:claims].pop
File.write(File.join(work, 'incomplete.jsonc'), JSON.pretty_generate(incomplete))
promoted = Marshal.load(Marshal.dump(data))
promoted[:claims][0][:candidate_status] = 'discharged'
File.write(File.join(work, 'promoted.jsonc'), JSON.pretty_generate(promoted))
RUBY

check_case() {
  local name="$1" expected="$2" marker="$3"
  shift 3
  local status=0
  bash "$REPO/scripts/ci/test-soak-claim-inventory.sh" "$@" >"$WORK/$name.log" 2>&1 || status=$?
  if [[ "$status" != "$expected" ]] || ! grep -Fq "$marker" "$WORK/$name.log"; then
    printf 'FAIL: Inventory case %s returned %s instead of the expected result.\n' "$name" "$status" >&2
    cat "$WORK/$name.log" >&2
    exit 1
  fi
  printf 'PASS: %s\n' "$name"
}

check_case explicit-json 0 'PASS: 4 required claims have current input digests' "$WORK/legacy.json"
check_case default-jsonc 0 'PASS: 4 required claims have current input digests'
check_case explicit-jsonc 0 'PASS: 4 required claims have current input digests' "$WORK/explicit.jsonc"
check_case unterminated-comment 1 'FAIL: The inventory is not valid JSONC:' "$WORK/unterminated.jsonc"
check_case invalid-value 1 'FAIL: The inventory is not valid JSONC:' "$WORK/invalid.jsonc"
check_case trailing-garbage 1 'FAIL: The inventory is not valid JSONC:' "$WORK/trailing-garbage.jsonc"
check_case stale-binding 1 'FAIL: The candidate input is stale:' "$WORK/stale.jsonc"
check_case missing-claim 1 'FAIL: The required claim inventory is incomplete or duplicated.' "$WORK/incomplete.jsonc"
check_case promoted-claim 1 'must retain its pending candidate obligations.' "$WORK/promoted.jsonc"
check_case missing-file 1 'FAIL: Required soak claims lack a candidate-bound inventory.' "$WORK/missing.jsonc"
printf 'PASS: All 10 JSONC inventory regression cases passed.\n'
