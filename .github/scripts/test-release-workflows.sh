#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
ruby -ryaml - \
	"$ROOT/.github/workflows/release.yml" \
	"$ROOT/.github/workflows/release-evidence.yml" \
	"$ROOT/.github/workflows/soak-in.yml" \
	"$ROOT/.github/workflows/canary-publish.yml" <<'RUBY'
def trigger(document)
  document["on"] || document[true]
end

def fail_if(condition, message)
  abort(message) if condition
end

release_path, evidence_path, soakin_path, canary_path = ARGV
release = YAML.load_file(release_path)
evidence = YAML.load_file(evidence_path)
soakin = YAML.load_file(soakin_path)
canary = YAML.load_file(canary_path)
release_trigger = trigger(release)
evidence_trigger = trigger(evidence)
soakin_trigger = trigger(soakin)
canary_trigger = trigger(canary)
fail_if(release_trigger.keys != ["workflow_dispatch"], "release promotion must be manual only")
fail_if(evidence_trigger.keys != ["workflow_dispatch"], "release evidence must be manual only")
fail_if(release["permissions"] != {}, "release promotion must have no permissions")
fail_if(evidence.dig("permissions", "actions") != "read", "release evidence must have read-only actions permission")
fail_if(evidence.dig("permissions", "contents") != "read", "release evidence must have read-only contents permission")
fail_if(release.fetch("jobs").keys != ["held"], "release promotion must contain only the held-state job")
fail_if(release.dig("jobs", "held", "permissions"), "held release job cannot add permissions")
generate = evidence.dig("jobs", "generate")
fail_if(!generate.is_a?(Hash), "release evidence generate job is missing")
expected_condition = "github.ref_name == github.event.repository.default_branch"
fail_if(generate["if"] != expected_condition, "release evidence must run from the default branch")
fail_if(generate.key?("environment"), "release evidence cannot use a protected environment")
external_uses = generate.fetch("steps").map { |step| step["uses"] }.compact.reject { |value| value.start_with?("./") }
fail_if(external_uses.empty?, "release evidence has no pinned actions")
fail_if(external_uses.any? { |value| !value.match?(/@[0-9a-f]{40}$/) }, "release evidence actions must use full commit SHAs")
evidence_text = File.read(evidence_path)
fail_if(!evidence_text.include?("/attempts/${run_attempt}/jobs"), "release evidence must read jobs from the recorded run attempt")

# Shard soak-in enrollment: held state, no permissions, dispatch + stable
# release publication only.
fail_if(soakin_trigger.keys.sort != ["release", "workflow_dispatch"], "soak-in must trigger on release and dispatch only")
fail_if(soakin_trigger.dig("release", "types") != ["published"], "soak-in release trigger must use types: [published]")
fail_if(soakin["permissions"] != {}, "soak-in must have no permissions")
fail_if(soakin.fetch("jobs").keys != ["held"], "soak-in must contain only the held-state job")
fail_if(soakin.dig("jobs", "held", "permissions"), "held soak-in job cannot add permissions")
soakin_text = File.read(soakin_path)
fail_if(!soakin_text.include?("prerelease"), "soak-in must gate out prereleases")

# Canary publisher: workflow_run from CI plus dispatch, least privilege,
# protected environment, pinned actions, and never a rebuild.
fail_if(canary_trigger.keys.sort != ["workflow_dispatch", "workflow_run"], "canary publish must trigger on workflow_run and dispatch only")
fail_if(canary_trigger.dig("workflow_run", "workflows") != ["CI"], "canary publish must follow the CI workflow")
fail_if(canary_trigger.dig("workflow_run", "types") != ["completed"], "canary publish must trigger on completed runs")
fail_if(canary["permissions"] != {}, "canary publish must default to no permissions")
publish = canary.dig("jobs", "publish")
fail_if(!publish.is_a?(Hash), "canary publish job is missing")
fail_if(publish.dig("permissions", "contents") != "write", "canary publish needs contents: write only at the job level")
fail_if(publish.dig("permissions", "actions") != "read", "canary publish needs actions: read at the job level")
fail_if(publish["environment"] != "protected-branch-image-publish", "canary publish must use the protected-branch-image-publish environment")
job_condition = publish["if"].to_s
%w[workflow_run.conclusion workflow_run.event head_branch].each do |fragment|
  fail_if(!job_condition.include?(fragment), "canary publish gate must check #{fragment}")
end
canary_uses = publish.fetch("steps").map { |step| step["uses"] }.compact.reject { |value| value.start_with?("./") }
fail_if(canary_uses.empty?, "canary publish has no pinned actions")
fail_if(canary_uses.any? { |value| !value.match?(/@[0-9a-f]{40}$/) }, "canary publish actions must use full commit SHAs")
canary_text = File.read(canary_path)
fail_if(!canary_text.include?("/attempts/${run_attempt}/jobs"), "canary publish must read jobs from the recorded run attempt")
fail_if(canary_text.match?(/docker\s+build\b|cargo\s+build\b|buildx\s+build\b/), "canary publish must never rebuild")
fail_if(canary_text.match?(/--force|-f\s+ref=refs\/tags.*--method\s+PATCH/), "canary publish must never move a tag")
fail_if(!canary_text.include?("--prerelease"), "canary publish must create a prerelease, not a release")

forbidden = [
  /contents:\s*write/,
  /packages:\s*write/,
  /git push/,
  /docker push/,
  /docker manifest push/,
  /create-github-app-token/,
  /release-action@/,
  /secrets\./,
]
[release_path, evidence_path, soakin_path].each do |path|
  text = File.read(path)
  forbidden.each do |pattern|
    fail_if(text.match?(pattern), "#{path} contains publishing or credential access: #{pattern.source}")
  end
end
fail_if(File.read(canary_path).match?(/create-github-app-token|release-action@/), "canary publish must not mint tokens or use release actions")
puts "release workflow tests passed"
RUBY
