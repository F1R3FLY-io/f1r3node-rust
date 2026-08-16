#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
ruby -ryaml - "$ROOT/.github/workflows/release.yml" "$ROOT/.github/workflows/release-evidence.yml" <<'RUBY'
def trigger(document)
  document["on"] || document[true]
end

def fail_if(condition, message)
  abort(message) if condition
end

release_path, evidence_path = ARGV
release = YAML.load_file(release_path)
evidence = YAML.load_file(evidence_path)
release_trigger = trigger(release)
evidence_trigger = trigger(evidence)
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
[release_path, evidence_path].each do |path|
  text = File.read(path)
  forbidden.each do |pattern|
    fail_if(text.match?(pattern), "#{path} contains publishing or credential access: #{pattern.source}")
  end
end
puts "release workflow tests passed"
RUBY
