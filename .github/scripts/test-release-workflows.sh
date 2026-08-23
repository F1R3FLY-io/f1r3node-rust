#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
ruby -ryaml - \
	"$ROOT/.github/workflows/release.yml" \
	"$ROOT/.github/workflows/release-evidence.yml" \
	"$ROOT/.github/workflows/soak-in.yml" \
	"$ROOT/.github/workflows/canary-publish.yml" \
	"$ROOT/.github/workflows/oci-validation.yml" \
	"$ROOT/.github/workflows/merge-recovery-soak.yml" <<'RUBY'
def trigger(document)
  document["on"] || document[true]
end

def fail_if(condition, message)
  abort(message) if condition
end

release_path, evidence_path, soakin_path, canary_path, oci_path, soak_path = ARGV
release = YAML.load_file(release_path)
evidence = YAML.load_file(evidence_path)
soakin = YAML.load_file(soakin_path)
canary = YAML.load_file(canary_path)
release_trigger = trigger(release)
evidence_trigger = trigger(evidence)
soakin_trigger = trigger(soakin)
canary_trigger = trigger(canary)
fail_if(evidence_trigger.keys != ["workflow_dispatch"], "release evidence must be manual only")
fail_if(evidence.dig("permissions", "actions") != "read", "release evidence must have read-only actions permission")
fail_if(evidence.dig("permissions", "contents") != "read", "release evidence must have read-only contents permission")

# Promotion controller (Phase 4): manual start plus automatic resume from the
# gate workflows, a read-only gates job, and a promote job that runs only on
# a promotable verdict under the protected release-credentials environment.
fail_if(release_trigger.keys.sort != ["workflow_dispatch", "workflow_run"], "release promotion must trigger on dispatch and gate workflow_run only")
fail_if(release_trigger.dig("workflow_run", "types") != ["completed"], "release promotion must resume on completed gate runs")
resume_from = release_trigger.dig("workflow_run", "workflows").to_a.sort
fail_if(resume_from != ["Full OCI Validation", "Merge Recovery Soak", "Slashing test suite"], "release promotion must resume from exactly the three gate workflows")
fail_if(release_trigger.dig("workflow_dispatch", "inputs", "candidate_tag", "required") != true, "release promotion dispatch must require candidate_tag")
fail_if(release["permissions"] != {}, "release promotion must default to no permissions")
fail_if(release.fetch("jobs").keys != ["gates", "promote"], "release promotion must contain the gates and promote jobs only")
gates = release.dig("jobs", "gates")
promote = release.dig("jobs", "promote")
fail_if(gates["permissions"] != {"actions" => "read", "contents" => "read"}, "gates job must be read-only")
fail_if(gates.key?("environment"), "gates job cannot use a protected environment")
fail_if(promote["environment"] != "release-credentials", "promote job must use the release-credentials environment")
fail_if(promote["needs"] != "gates", "promote job must depend on the gates job")
fail_if(promote["if"].to_s != "needs.gates.outputs.promotable == 'true'", "promote job must run only on a promotable verdict")
fail_if(promote.dig("permissions", "contents") != "write", "promote job needs contents: write at the job level")
fail_if(promote.dig("permissions", "pull-requests") != "write", "promote job needs pull-requests: write for the next-version pull request")
fail_if(promote.dig("permissions", "actions"), "promote job must not read Actions; the gates job supplies the evaluation")
[gates, promote].each do |job|
  checkout = job.fetch("steps").first
  fail_if(checkout["uses"].to_s !~ /\Aactions\/checkout@[0-9a-f]{40}\z/, "release jobs must start with a pinned checkout")
  fail_if(checkout.dig("with", "ref") != "${{ github.event.repository.default_branch }}", "release jobs must check out trusted controls from the default branch")
  fail_if(checkout.dig("with", "persist-credentials") != false, "release checkouts must not persist credentials")
  uses = job.fetch("steps").map { |step| step["uses"] }.compact.reject { |value| value.start_with?("./") }
  fail_if(uses.any? { |value| !value.match?(/@[0-9a-f]{40}$/) }, "release promotion actions must use full commit SHAs")
end
release_text = File.read(release_path)
fail_if(release_text.match?(/docker\s+build\b|cargo\s+build\b|buildx\s+build\b|docker\s+push\b/), "release promotion must never rebuild or push layers")
fail_if(release_text.match?(/--force|-f\s+ref=refs\/tags.*--method\s+PATCH|git\s+push\s+.*(--force|-f\b)/), "release promotion must never move a tag")
fail_if(!release_text.include?("imagetools create"), "release promotion must copy the candidate image by digest")
fail_if(!release_text.include?("--verify-tag"), "release promotion must create the release on the verified tag")
fail_if(release_text.include?("--prerelease"), "release promotion must create a stable release, not a prerelease")
fail_if(!release_text.include?("release-gates.sh evaluate"), "release promotion must evaluate gates with release-gates.sh")
fail_if(!release_text.include?("promote-release.sh plan"), "release promotion must execute a promote-release.sh plan")
fail_if(!release_text.include?("promote-release.sh verify-binaries"), "release promotion must verify candidate binaries before publishing")
gates_text = release_text[/  gates:.*?\n  promote:/m].to_s
fail_if(gates_text.match?(/secrets\.(?!GITHUB_TOKEN)/), "gates job may use only the GITHUB_TOKEN")
fail_if(gates_text.match?(/docker login|gh release create|git push|refs\/tags.*-f sha/), "gates job must not publish")
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
[evidence_path, soakin_path].each do |path|
  text = File.read(path)
  forbidden.each do |pattern|
    fail_if(text.match?(pattern), "#{path} contains publishing or credential access: #{pattern.source}")
  end
end
fail_if(File.read(canary_path).match?(/create-github-app-token|release-action@/), "canary publish must not mint tokens or use release actions")
fail_if(release_text.match?(/create-github-app-token|release-action@/), "release promotion must not mint tokens or use release actions")
# Gate workflows (Phase 3): candidate mode publishes section 8.1 documents
# from one job that alone holds contents: write under release-credentials,
# and the candidate image is pulled by digest, never rebuilt, in that mode.
canary_text = File.read(canary_path)
fail_if(!canary_text.include?("Publish canary images to OCIR"), "canary publish must dual-publish the index to OCIR")
fail_if(!canary_text.match?(/record-images[^\n]*\\\n(?:[^\n]*\n){4}[^\n]*ocir-index-digest/m), "canary publish must record the OCIR index digest")
{ "oci-validation" => oci_path, "merge-recovery-soak" => soak_path }.each do |label, path|
  doc = YAML.load_file(path)
  publish = doc.dig("jobs", "publish_candidate_evidence")
  fail_if(!publish.is_a?(Hash), "#{label} must define the publish_candidate_evidence job")
  fail_if(publish["environment"] != "release-credentials", "#{label} gate publication must use the release-credentials environment")
  fail_if(publish.dig("permissions", "contents") != "write", "#{label} gate publication needs contents: write at the job level")
  fail_if(publish["if"].to_s !~ /candidate_tag != ''/, "#{label} gate publication must run only in candidate mode")
  doc.fetch("jobs").each do |name, job|
    next if name == "publish_candidate_evidence"
    fail_if(job.dig("permissions", "contents") == "write", "#{label} job #{name} must not hold contents: write")
  end
  text = File.read(path)
  fail_if(!text.include?("release-gate-evidence.sh"), "#{label} must write gate documents with release-gate-evidence.sh")
  fail_if(!text.include?("name: release-candidate"), "#{label} must upload the release-candidate marker artifact")
end
# The OCI image steps live in the reusable workflow; the soak's live inline.
reusable_text = File.read("#{File.dirname(oci_path)}/reusable-oci-validation.yml")
fail_if(!reusable_text.match?(/docker pull --platform [^\n]*@\$\{ARCH_DIGEST\}/), "OCI validation candidate mode must pull the image by digest")
fail_if(!reusable_text.match?(/Build Docker Image\n\s+if: inputs\.candidate_tag == ''/m), "OCI validation must skip the source build in candidate mode")
soak_text = File.read(soak_path)
fail_if(!soak_text.match?(/docker pull --platform [^\n]*@\$\{amd64_digest\}/), "soak candidate mode must pull the image by digest")
fail_if(!soak_text.match?(/Build node image\n\s+if: needs\.schedule_gate\.outputs\.candidate_tag == ''/m), "soak must skip the source build in candidate mode")
puts "release workflow tests passed"
RUBY
