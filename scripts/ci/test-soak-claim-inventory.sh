#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
INVENTORY="${1:-$ROOT/docs/claims/soak-claim-inventory.jsonc}"

ruby -rjson -rdigest - "$ROOT" "$INVENTORY" <<'RUBY'
root, inventory = ARGV
Dir.chdir(root)

def require_value(condition, message)
  abort "FAIL: #{message}" unless condition
end

def text(value)
  value.is_a?(String) && !value.strip.empty?
end

def list(value)
  value.is_a?(Array) && !value.empty? && value.all? { |item| text(item) }
end

def fields(record, names, label)
  require_value(record.is_a?(Hash), "#{label} must be an object.")
  names.each { |name| require_value(record.key?(name), "#{label} lacks #{name}.") }
end

def input_file(path, inputs)
  require_value(text(path) && !path.start_with?('/') && !path.split('/').include?('..'), 'Input paths must stay inside the repository.')
  require_value(inputs.key?(path), "#{path} lacks a candidate digest.")
end

required_claims = %w[CLAIM-FINALITY-001 CLAIM-FINALITY-002 CLAIM-SOAK-GATE-001 CLAIM-SOAK-001]
require_value(File.file?(inventory), 'Required soak claims lack a candidate-bound inventory.')
begin
  data = JSON.parse(File.read(inventory))
rescue JSON::ParserError => error
  abort "FAIL: The inventory is not valid JSONC: #{error.message}"
end
fields(data, %w[schema_version candidate claims checks evidence gates acceptance], 'Inventory')
require_value(data['schema_version'] == 1, 'The inventory schema is unsupported.')
fields(data['candidate'], %w[base_commit binding inputs_sha256], 'Candidate')
require_value(data['candidate']['base_commit'].is_a?(String) && data['candidate']['base_commit'].match?(/\A[0-9a-f]{40}\z/), 'The candidate base must use a full commit SHA.')
require_value(data['candidate']['binding'] == 'source-sha256', 'The candidate must bind source content.')
inputs = data['candidate']['inputs_sha256']
require_value(inputs.is_a?(Hash) && !inputs.empty?, 'The candidate input set is empty.')
inputs.each do |path, digest|
  input_file(path, inputs)
  require_value(File.file?(path), "The candidate input is missing: #{path}.")
  require_value(digest.is_a?(String) && digest.match?(/\A[0-9a-f]{64}\z/), "The input digest is malformed: #{path}.")
  require_value(Digest::SHA256.file(path).hexdigest == digest, "The candidate input is stale: #{path}.")
end

claims = data['claims']
checks = data['checks']
evidence = data['evidence']
require_value(claims.is_a?(Array) && checks.is_a?(Array) && evidence.is_a?(Hash), 'The claim, check, and evidence collections are invalid.')
require_value(claims.all? { |claim| claim.is_a?(Hash) }, 'Each claim must be an object.')
require_value(claims.map { |claim| claim['id'] }.sort == required_claims.sort, 'The required claim inventory is incomplete or duplicated.')
check_ids = checks.map do |check|
  fields(check, %w[id command configurations verifier assumptions bounds production_tests ci evidence_refs result pending_obligations], 'Check')
  require_value(text(check['id']) && text(check['command']), 'Each check must identify its command.')
  require_value(list(check['assumptions']) && list(check['bounds']) && list(check['pending_obligations']), "#{check['id']} lacks its scope or pending obligations.")
  require_value(list(check['configurations']), "#{check['id']} lacks its configuration.")
  check['configurations'].each { |path| input_file(path, inputs) }
  fields(check['verifier'], %w[name version], check['id'])
  require_value(text(check['verifier']['name']) && text(check['verifier']['version']), "#{check['id']} lacks its verifier identity.")
  require_value(check['production_tests'].is_a?(Array) && !check['production_tests'].empty?, "#{check['id']} lacks its production test mapping.")
  check['production_tests'].each do |test|
    fields(test, %w[path command result], check['id'])
    input_file(test['path'], inputs)
    require_value(text(test['command']) && text(test['result']), "#{check['id']} has an incomplete production test mapping.")
  end
  fields(check['ci'], %w[workflow job enforcement], check['id'])
  input_file(check['ci']['workflow'], inputs)
  require_value(text(check['ci']['job']) && text(check['ci']['enforcement']), "#{check['id']} lacks its CI obligation.")
  require_value(%w[historical-baseline not-run].include?(check['result']), 'Inventory verification must not promote baseline checks to candidate acceptance.')
  require_value(list(check['evidence_refs']) && check['evidence_refs'].all? { |id| evidence.key?(id) }, "#{check['id']} has missing evidence references.")
  check['id']
end
require_value(check_ids.uniq == check_ids, 'Check identifiers must be unique.')
claims.each do |claim|
  fields(claim, %w[id declared_status candidate_status specification implementation checks pending_obligations], 'Claim')
  input_file(claim['specification'], inputs)
  require_value(text(claim['declared_status']) && claim['candidate_status'] == 'pending', "#{claim['id']} must retain its pending candidate obligations.")
  require_value(claim['implementation'].is_a?(Array) && !claim['implementation'].empty?, "#{claim['id']} lacks its implementation.")
  claim['implementation'].each do |implementation|
    fields(implementation, %w[path symbols], claim['id'])
    input_file(implementation['path'], inputs)
    require_value(list(implementation['symbols']), "#{claim['id']} lacks its production symbols.")
  end
  require_value(list(claim['checks']) && (claim['checks'] - check_ids).empty?, "#{claim['id']} lacks required checks.")
  require_value(list(claim['pending_obligations']), "#{claim['id']} lacks pending obligations.")
end

evidence.each do |id, record|
  fields(record, %w[path role identity limitations], id)
  input_file(record['path'], inputs)
  require_value(%w[historical-baseline incident pending-plan hosted-baseline].include?(record['role']), "#{id} has an unsupported evidence role.")
  require_value(text(record['identity']) && list(record['limitations']), "#{id} lacks evidence identity or limitations.")
end
require_value(data['gates'].is_a?(Array) && data['gates'].all? { |gate| gate.is_a?(Hash) }, 'The gate inventory is invalid.')
require_value(data['gates'].map { |gate| gate['id'] }.sort == %w[G0 D1 D2 O1 F1 F2 F3 D3 A1].sort, 'The gate inventory is incomplete or duplicated.')
data['gates'].each do |gate|
  fields(gate, %w[id status checks pending_obligations], 'Gate')
  require_value(gate['status'] == 'pending' && list(gate['pending_obligations']), "#{gate['id']} must retain its pending obligations.")
  require_value(list(gate['checks']) && (gate['checks'] - check_ids).empty?, "#{gate['id']} lacks its check mapping.")
end
fields(data['acceptance'], %w[status identity pending_obligations], 'Acceptance')
require_value(data['acceptance']['status'] == 'pending' && list(data['acceptance']['pending_obligations']), 'Soak acceptance remains pending.')
identity = data['acceptance']['identity']
fields(identity, %w[workflow_control_sha node_sha image_digest harness_sha configuration workload_profile run_id run_attempt], 'Acceptance identity')
require_value(identity.values.all?(&:nil?), 'This inventory does not certify a soak candidate.')
puts "PASS: #{claims.length} required claims have current input digests and explicit pending obligations."
puts 'Inventory validity is not formal discharge or soak acceptance.'
RUBY
