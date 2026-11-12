#!/usr/bin/env bash
set -euo pipefail

ROOT="${1:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

ruby -ryaml - "$ROOT" "$TMP" <<'RUBY'
root, temporary = ARGV
load_workflow = ->(name) { YAML.load_file(File.join(root, ".github/workflows", name)) }
events = ->(workflow) { workflow["on"] || workflow[true] || {} }
checkout = ->(step) { step["uses"].to_s.start_with?("actions/checkout@") }
ci = load_workflow.("ci.yml")
triggers = events.(ci)
abort "ordinary CI must support pull_request and workflow_dispatch" unless %w[pull_request workflow_dispatch].all? { |event| triggers.key?(event) }
abort "ordinary CI must not run as pull_request_target" if triggers.key?("pull_request_target")

ci.fetch("jobs").each do |name, job|
  Array(job["steps"]).select(&checkout).each do |step|
    ref = step.dig("with", "ref")
    expected = name == "build_base" ? "${{ steps.target.outputs.target_sha }}" : "${{ needs.build_base.outputs.TARGET_SHA }}"
    abort "#{name} must not persist checkout credentials" unless step.dig("with", "persist-credentials") == false
    next if name == "promotion_source" && ref.nil?
    if %w[release_docker_image release_packages].include?(name) && ref.nil?
      abort "#{name} must remain restricted to push events" unless job["if"].to_s.include?("github.event_name == 'push'")
      next
    end
    abort "#{name} must check out the resolved event or exact-target revision" unless ref == expected
  end
end
pipeline = ci.dig("jobs", "pipeline")
abort "CI must call the reusable workflow from its own revision" unless pipeline["uses"] == "./.github/workflows/_integration-pipeline.yml"
abort "the reusable pipeline must receive the resolved target" unless pipeline.dig("with", "checkout_ref") == "${{ needs.build_base.outputs.TARGET_SHA }}"
abort "ordinary CI must not receive promotion app credentials" if ci.to_s.match?(/PROMOTION_APP_(ID|PRIVATE_KEY)/)
abort "workflow revision tests must run in Script Tests" unless ci.dig("jobs", "script_tests", "steps").any? { |step| step["run"].to_s.include?("bash .github/scripts/test-ci-workflow-refs.sh") }

integration = load_workflow.("_integration-pipeline.yml")
abort "the integration pipeline must not receive promotion app credentials" if integration.to_s.match?(/PROMOTION_APP_(ID|PRIVATE_KEY)/)
integration.fetch("jobs").each do |name, job|
  Array(job["steps"]).select(&checkout).each do |step|
    expected = step.dig("with", "repository") == "F1R3FLY-io/system-integration" ?
      "${{ env.SYSTEM_INTEGRATION_REF }}" : "${{ inputs.checkout_ref != '' && inputs.checkout_ref || github.sha }}"
    abort "#{name} must retain the selected code revision or pinned trusted harness" unless step.dig("with", "ref") == expected
    abort "#{name} must not persist checkout credentials" unless step.dig("with", "persist-credentials") == false
  end
end

%w[protected-head-guard.yml ci-heavy-label.yml].each do |name|
  workflow = load_workflow.(name)
  abort "#{name} must execute trusted pull_request_target code" unless events.(workflow).keys == ["pull_request_target"]
  workflow.fetch("jobs").each_value do |job|
    Array(job["steps"]).select(&checkout).each do |step|
      ref = step.dig("with", "ref")
      abort "#{name} must fetch trusted base scripts" unless ref.nil? || ref == "${{ github.event.pull_request.base.sha }}"
      abort "#{name} must not persist checkout credentials" unless step.dig("with", "persist-credentials") == false
    end
  end
end

%w[promote-and-backmerge.yml ca-daily-sync.yml].each do |name|
  workflow = load_workflow.(name)
  workflow.fetch("jobs").each_value do |job|
    abort "#{name} must use release-credentials" unless job["environment"] == "release-credentials"
    steps = job.fetch("steps")
    guard = steps.first
    abort "#{name} must check the actual dispatch ref first" unless guard.dig("env", "REF") == "${{ github.ref }}"
    body = guard.fetch("run")
    abort "#{name} must reject refs outside master" unless body.include?("refs/heads/master") && body.include?("exit 1")
    Array(steps).select(&checkout).each do |step|
      abort "#{name} must retain trusted master scripts" unless step.dig("with", "ref").nil? || step.dig("with", "ref") == "master"
      abort "#{name} must not persist checkout credentials" unless step.dig("with", "persist-credentials") == false
    end
    File.write(File.join(temporary, name + ".guard.sh"), body)
  end
end

train = load_workflow.("deployment-train.yml")
daily = load_workflow.("ca-daily-sync.yml")
open_backmerge = daily.dig("jobs", "sync", "steps").find { |step| step["name"] == "Open the back-merge pull request" }
abort "only the schedule event may mark a back-merge automatic" unless open_backmerge.dig("env", "BACKMERGE_SCHEDULED") == "${{ github.event_name == 'schedule' && '1' || '0' }}"
abort "manual daily back-merges must name their initiator" unless open_backmerge.dig("env", "BACKMERGE_TRIGGERED_BY") == "${{ github.event_name == 'workflow_dispatch' && github.triggering_actor || '' }}"
abort "the opener must not force scheduled mode for manual dispatches" if open_backmerge["run"].match?(/BACKMERGE_SCHEDULED\s*=/)
controls = train.fetch("jobs").values.flat_map { |job| Array(job["steps"]) }.find { |step| checkout.(step) && step.dig("with", "path") == "ci-control" }
abort "exact-merge release dispatch must retain trusted default-branch controls" unless controls&.dig("with", "ref") == "${{ github.event.repository.default_branch }}"
dispatch = train.fetch("jobs").values.flat_map { |job| Array(job["steps"]) }.map { |step| step["run"].to_s }.join("\n")
abort "exact-merge release dispatch must retain separate workflow and candidate revisions" unless dispatch.include?('gh workflow run ci.yml --ref "$DEFAULT_BRANCH"') && dispatch.include?('-f target_sha="$merge_sha"')
RUBY

for guard in "$TMP"/*.guard.sh; do
	REF=refs/heads/master bash -euo pipefail "$guard"
	for rejected_ref in refs/heads/ci/workflow-preview refs/heads/dev refs/heads/master-preview refs/tags/master; do
		if REF="$rejected_ref" bash -euo pipefail "$guard" >"$TMP/rejected.log" 2>&1; then
			printf 'FAIL: %s reached protected promotion execution\n' "$rejected_ref" >&2
			exit 1
		fi
	done
done

printf 'PASS: CI uses the selected revision, local reusable workflows follow their caller, and promotion credentials remain restricted to master.\n'
